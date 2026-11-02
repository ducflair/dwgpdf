use crate::error::{ConversionError, ConversionResult};
use crate::options::ConversionOptions;
use hipdf::images::{Image, ImageManager};
use hipdf::lopdf::content::{Content, Operation};
use hipdf::lopdf::{Dictionary, Document, Object, Stream};
use roxmltree::Document as XmlDocument;
use std::io::{Cursor, Read};
use zip::ZipArchive;

#[derive(Debug)]
#[allow(dead_code)]
struct DwfSheet {
    title: String,
    image_bytes: Option<(Vec<u8>, String)>, // (bytes, extension)
}

/// Converts a DWF package to PDF bytes using hipdf.
pub fn convert_dwf_to_pdf(dwf_bytes: &[u8], options: &ConversionOptions) -> ConversionResult<Vec<u8>> {
    let cursor = Cursor::new(dwf_bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|e| {
        ConversionError::DwfParse(format!("Failed to open DWF zip archive: {}", e))
    })?;

    // 1. Read manifest.xml
    let mut manifest_xml = String::new();
    let mut title_from_manifest = None;

    if let Ok(mut manifest_file) = archive.by_name("manifest.xml") {
        manifest_file.read_to_string(&mut manifest_xml).ok();
    }

    let mut sheets: Vec<DwfSheet> = Vec::new();

    if !manifest_xml.is_empty() {
        if let Ok(xml_doc) = XmlDocument::parse(&manifest_xml) {
            for node in xml_doc.descendants() {
                if node.has_tag_name("Section") {
                    let section_title = node
                        .attribute("title")
                        .unwrap_or("Untitled Sheet")
                        .to_string();

                    if title_from_manifest.is_none() {
                        title_from_manifest = Some(section_title.clone());
                    }

                    // Find child resources for thumbnail or graphic
                    let mut sheet_image = None;
                    for child in node.descendants() {
                        if child.has_tag_name("Resource") {
                            let role = child.attribute("role").unwrap_or("");
                            let mime = child.attribute("mime").unwrap_or("");
                            let href = child.attribute("href").unwrap_or("");

                            if (role == "thumbnail" || role == "preview" || mime.starts_with("image/"))
                                && !href.is_empty()
                            {
                                // Normalize href path separators
                                let zip_path = href.replace('\\', "/");
                                let zip_path_win = href.replace('/', "\\");

                                let zip_idx = archive
                                    .index_for_name(&zip_path)
                                    .or_else(|| archive.index_for_name(&zip_path_win));

                                let file_data = if let Some(idx) = zip_idx {
                                    archive.by_index(idx).ok().and_then(|mut f| {
                                        let mut buf = Vec::new();
                                        f.read_to_end(&mut buf).ok().map(|_| buf)
                                    })
                                } else {
                                    None
                                };

                                if let Some(bytes) = file_data {
                                    let ext = if mime == "image/jpeg" || href.ends_with(".jpg") {
                                        "jpg".to_string()
                                    } else {
                                        "png".to_string()
                                    };
                                    sheet_image = Some((bytes, ext));
                                    break;
                                }
                            }
                        }
                    }

                    sheets.push(DwfSheet {
                        title: section_title,
                        image_bytes: sheet_image,
                    });
                }
            }
        }
    }

    // Fallback: If no sheets found in manifest, scan archive for any jpg/png images
    if sheets.is_empty() || sheets.iter().all(|s| s.image_bytes.is_none()) {
        for i in 0..archive.len() {
            if let Ok(mut f) = archive.by_index(i) {
                let name = f.name().to_string();
                let lower = name.to_lowercase();
                if lower.ends_with(".jpg") || lower.ends_with(".png") || lower.ends_with(".jpeg") {
                    let mut bytes = Vec::new();
                    if f.read_to_end(&mut bytes).is_ok() && bytes.len() > 1024 {
                        let ext = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
                            "jpg".to_string()
                        } else {
                            "png".to_string()
                        };
                        sheets.push(DwfSheet {
                            title: format!("Sheet {}", sheets.len() + 1),
                            image_bytes: Some((bytes, ext)),
                        });
                        break;
                    }
                }
            }
        }
    }

    if sheets.is_empty() {
        return Err(ConversionError::EmptyDrawing);
    }

    // Build PDF
    let mut pdf_doc = Document::with_version("1.7");
    let mut page_ids = Vec::new();

    for sheet in &sheets {
        let (page_width, page_height) = (841.89, 595.28); // Standard A4 Landscape in points
        let mut image_manager = ImageManager::new();
        let mut resources = Dictionary::new();
        let mut content_ops = Vec::new();

        // Background color
        content_ops.push(Operation::new("q", vec![]));
        content_ops.push(Operation::new("rg", vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)]));
        content_ops.push(Operation::new(
            "re",
            vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(page_width as f32),
                Object::Real(page_height as f32),
            ],
        ));
        content_ops.push(Operation::new("f", vec![]));
        content_ops.push(Operation::new("Q", vec![]));

        if let Some((img_bytes, ext)) = &sheet.image_bytes {
            let img = Image::from_bytes(img_bytes.clone(), Some(format!("dwf_img.{}", ext))).map_err(|e| {
                ConversionError::ResourceLoad(format!("Failed to load DWF image: {}", e))
            })?;

            let (img_w, img_h) = img.dimensions();
            let image_id = image_manager.embed_image(&mut pdf_doc, img).map_err(|e| {
                ConversionError::PdfGeneration(format!("Failed to embed DWF image: {}", e))
            })?;

            // Fit image on page with margins
            let margin = options.margin_pt.max(20.0);
            let target_w = page_width - 2.0 * margin;
            let target_h = page_height - 2.0 * margin;

            let scale_x = target_w / (img_w as f64);
            let scale_y = target_h / (img_h as f64);
            let scale = scale_x.min(scale_y);

            let draw_w = (img_w as f64) * scale;
            let draw_h = (img_h as f64) * scale;
            let draw_x = margin + (target_w - draw_w) / 2.0;
            let draw_y = margin + (target_h - draw_h) / 2.0;

            let res_name = image_manager.add_to_resources(&mut resources, image_id);
            let draw_ops = ImageManager::draw_image(
                &res_name,
                draw_x as f32,
                draw_y as f32,
                draw_w as f32,
                draw_h as f32,
            );

            content_ops.extend(draw_ops);
        }

        let content = Content { operations: content_ops };
        let content_data = content.encode().map_err(|e| {
            ConversionError::PdfGeneration(format!("Encode content error: {}", e))
        })?;
        let stream = Stream::new(Dictionary::new(), content_data);
        let content_id = pdf_doc.add_object(Object::Stream(stream));

        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set(
            "MediaBox",
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(page_width as f32),
                Object::Real(page_height as f32),
            ]),
        );
        page.set("Contents", Object::Reference(content_id));
        page.set("Resources", Object::Dictionary(resources));

        let page_id = pdf_doc.add_object(Object::Dictionary(page));
        page_ids.push(page_id);
    }

    // Pages Tree
    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Count", Object::Integer(page_ids.len() as i64));
    pages.set("Kids", Object::Array(page_ids.iter().map(|id| Object::Reference(*id)).collect()));

    let pages_id = pdf_doc.add_object(Object::Dictionary(pages));

    for page_id in &page_ids {
        if let Ok(page_obj) = pdf_doc.get_object_mut(*page_id) {
            if let Object::Dictionary(ref mut dict) = page_obj {
                dict.set("Parent", Object::Reference(pages_id));
            }
        }
    }

    // Root Catalog
    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    let catalog_id = pdf_doc.add_object(Object::Dictionary(catalog));
    pdf_doc.trailer.set("Root", Object::Reference(catalog_id));

    // Info
    let mut info = Dictionary::new();
    let title = options
        .metadata
        .title
        .clone()
        .or(title_from_manifest)
        .unwrap_or_else(|| "DWF Drawing".to_string());
    info.set("Title", Object::string_literal(title));
    info.set("Creator", Object::string_literal("dwgpdf"));
    let info_id = pdf_doc.add_object(Object::Dictionary(info));
    pdf_doc.trailer.set("Info", Object::Reference(info_id));

    let mut output = Vec::new();
    pdf_doc.save_to(&mut output).map_err(|e| {
        ConversionError::PdfGeneration(format!("Save PDF error: {}", e))
    })?;

    Ok(output)
}
