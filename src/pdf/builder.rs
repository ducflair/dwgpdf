use crate::cad::bounds::{calculate_document_bounds, BoundingBox, Transform2D};
use crate::cad::color::{parse_hex_color, resolve_color, rgb_to_pdf};
use crate::cad::mtext::{clean_mtext, decode_cad_text_escapes};
use crate::error::{ConversionError, ConversionResult};
use crate::options::{ColorMode, ConversionMode, ConversionOptions, Orientation, PageSize};
use crate::pdf::geometry::{
    generate_arc_path, generate_bulge_segment, generate_circle_path, generate_ellipse_path, Point2D,
};

use acadrust::document::CadDocument;
use acadrust::entities::EntityType;
use acadrust::types::color::Color;

use hipdf::fonts::{Font, FontManager, StandardFont};
use hipdf::lopdf::content::{Content, Operation};
use hipdf::lopdf::{Dictionary, Document, Object, Stream};
use hipdf::ocg::OCGManager;

use std::collections::{HashMap, HashSet};

pub struct CadToPdfBuilder<'a> {
    doc: &'a CadDocument,
    options: ConversionOptions,
    bbox: BoundingBox,
    pdf_doc: Document,
    ocg_manager: OCGManager,
    font_manager: FontManager,
    primary_font_id: (u32, u16),
    primary_font_name: String,
    scale: f64,
    tx: f64,
    ty: f64,
    page_width: f64,
    page_height: f64,
    layer_colors: HashMap<String, Color>,
    default_draw_rgb: (u8, u8, u8),
}

impl<'a> CadToPdfBuilder<'a> {
    pub fn new(doc: &'a CadDocument, options: ConversionOptions) -> ConversionResult<Self> {
        let is_model_space = options.layout.as_deref().unwrap_or("Model").eq_ignore_ascii_case("model");
        let doc_bounds = calculate_document_bounds(doc, is_model_space);

        let bbox = match &options.mode {
            ConversionMode::Plot => doc_bounds,
            ConversionMode::Crop {
                offset_x,
                offset_y,
                width,
                height,
            } => {
                let w = width.unwrap_or(doc_bounds.width());
                let h = height.unwrap_or(doc_bounds.height());
                BoundingBox {
                    min_x: *offset_x,
                    min_y: *offset_y,
                    max_x: *offset_x + w,
                    max_y: *offset_y + h,
                }
            }
        };

        let default_draw_rgb = options
            .default_draw_color
            .as_deref()
            .and_then(parse_hex_color)
            .unwrap_or((0, 0, 0));

        let mut layer_colors = HashMap::new();
        for layer in doc.layers.iter() {
            layer_colors.insert(layer.name.clone(), layer.color.clone());
        }

        let mut pdf_doc = Document::with_version("1.7");
        let mut font_manager = FontManager::new();

        let helvetica = Font::standard(StandardFont::Helvetica);
        let (primary_font_id, primary_font_name) = font_manager
            .embed_font(&mut pdf_doc, helvetica)
            .map_err(|e| ConversionError::PdfGeneration(format!("Font embedding error: {}", e)))?;

        let margin = options.margin_pt;
        let (page_w, page_h, scale, tx, ty) = Self::compute_page_and_transform(&bbox, &options, margin);

        Ok(Self {
            doc,
            options,
            bbox,
            pdf_doc,
            ocg_manager: OCGManager::new(),
            font_manager,
            primary_font_id,
            primary_font_name,
            scale,
            tx,
            ty,
            page_width: page_w,
            page_height: page_h,
            layer_colors,
            default_draw_rgb,
        })
    }

    pub fn bbox(&self) -> &BoundingBox {
        &self.bbox
    }

    fn compute_page_and_transform(
        bbox: &BoundingBox,
        options: &ConversionOptions,
        margin: f64,
    ) -> (f64, f64, f64, f64, f64) {
        let drawing_w = bbox.width().max(1.0);
        let drawing_h = bbox.height().max(1.0);

        let (page_w, page_h) = match options.page_size {
            PageSize::Auto => {
                let mut w = drawing_w + 2.0 * margin;
                let mut h = drawing_h + 2.0 * margin;

                match options.orientation {
                    Orientation::Portrait => {
                        if w > h {
                            std::mem::swap(&mut w, &mut h);
                        }
                    }
                    Orientation::Landscape => {
                        if h > w {
                            std::mem::swap(&mut w, &mut h);
                        }
                    }
                    Orientation::Auto => {}
                }
                (w.max(50.0), h.max(50.0))
            }
            standard_size => {
                let (mut w, mut h) = standard_size.dimensions_pt().unwrap_or((595.28, 841.89));
                let drawing_aspect = drawing_w / drawing_h;
                let is_drawing_landscape = drawing_aspect > 1.0;

                match options.orientation {
                    Orientation::Auto => {
                        if is_drawing_landscape && h > w {
                            std::mem::swap(&mut w, &mut h);
                        } else if !is_drawing_landscape && w > h {
                            std::mem::swap(&mut w, &mut h);
                        }
                    }
                    Orientation::Landscape => {
                        if h > w {
                            std::mem::swap(&mut w, &mut h);
                        }
                    }
                    Orientation::Portrait => {
                        if w > h {
                            std::mem::swap(&mut w, &mut h);
                        }
                    }
                }
                (w, h)
            }
        };

        let printable_w = (page_w - 2.0 * margin).max(1.0);
        let printable_h = (page_h - 2.0 * margin).max(1.0);

        let scale = if let Some(user_scale) = options.scale {
            user_scale
        } else {
            let scale_x = printable_w / drawing_w;
            let scale_y = printable_h / drawing_h;
            scale_x.min(scale_y)
        };

        let content_w = drawing_w * scale;
        let content_h = drawing_h * scale;

        let tx = margin + (printable_w - content_w) / 2.0 - bbox.min_x * scale;
        let ty = margin + (printable_h - content_h) / 2.0 - bbox.min_y * scale;

        (page_w, page_h, scale, tx, ty)
    }

    pub fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.scale + self.tx, y * self.scale + self.ty)
    }

    pub fn build(mut self) -> ConversionResult<Vec<u8>> {
        let mut content_ops = Vec::new();

        // 1. Background color
        if let Some(bg_color_str) = &self.options.background_color {
            if let Some(rgb) = parse_hex_color(bg_color_str) {
                let (r, g, b) = rgb_to_pdf(rgb);
                content_ops.push(Operation::new("q", vec![]));
                content_ops.push(Operation::new(
                    "rg",
                    vec![Object::Real(r), Object::Real(g), Object::Real(b)],
                ));
                content_ops.push(Operation::new(
                    "re",
                    vec![
                        Object::Real(0.0),
                        Object::Real(0.0),
                        Object::Real(self.page_width as f32),
                        Object::Real(self.page_height as f32),
                    ],
                ));
                content_ops.push(Operation::new("f", vec![]));
                content_ops.push(Operation::new("Q", vec![]));
            }
        }

        // 2. Set default graphics state
        content_ops.push(Operation::new("q", vec![]));
        content_ops.push(Operation::new("1", vec![])); // 1 J
        content_ops.push(Operation::new("1", vec![])); // 1 j

        let is_model_space = self
            .options
            .layout
            .as_deref()
            .unwrap_or("Model")
            .eq_ignore_ascii_case("model");

        let entities: Vec<&EntityType> = if is_model_space {
            self.doc.model_space_entities().collect()
        } else {
            self.doc.entities().collect()
        };

        let mut entities_by_layer: HashMap<String, Vec<&EntityType>> = HashMap::new();
        for entity in entities {
            let layer_name = entity.as_entity().layer().to_string();
            entities_by_layer.entry(layer_name).or_default().push(entity);
        }

        let mut layer_ocg_map: HashMap<String, String> = HashMap::new();

        // Render each layer
        for (layer_name, layer_entities) in entities_by_layer {
            if let Some(layer) = self.doc.layers.iter().find(|l| l.name == layer_name) {
                if layer.is_frozen() || layer.is_off() {
                    continue;
                }
            }

            let mut layer_stream = String::new();
            let mut visited_blocks = HashSet::new();

            for entity in layer_entities {
                self.render_entity(
                    entity,
                    &Transform2D::identity(),
                    &mut layer_stream,
                    &mut visited_blocks,
                    0,
                );
            }

            if layer_stream.is_empty() {
                continue;
            }

            if self.options.export_layers {
                let ocg_prop_name = format!("OCG_{}", layer_ocg_map.len() + 1);
                self.ocg_manager.add_layer(hipdf::ocg::Layer::new(&layer_name, true));
                layer_ocg_map.insert(layer_name.clone(), ocg_prop_name.clone());

                content_ops.push(Operation::new(
                    "/OC",
                    vec![Object::Name(ocg_prop_name.into_bytes())],
                ));
                if let Ok(c) = Content::decode(layer_stream.as_bytes()) {
                    content_ops.extend(c.operations);
                }
                content_ops.push(Operation::new("EMC", vec![]));
            } else {
                if let Ok(c) = Content::decode(layer_stream.as_bytes()) {
                    content_ops.extend(c.operations);
                }
            }
        }

        content_ops.push(Operation::new("Q", vec![]));

        let content = Content { operations: content_ops };
        let content_data = content.encode().map_err(|e| {
            ConversionError::PdfGeneration(format!("Content encode error: {}", e))
        })?;

        let stream = Stream::new(Dictionary::new(), content_data);
        let content_id = self.pdf_doc.add_object(Object::Stream(stream));

        let mut resources = Dictionary::new();
        self.font_manager.add_to_resources(&mut resources, self.primary_font_id, &self.primary_font_name);

        if self.options.export_layers && !layer_ocg_map.is_empty() {
            self.ocg_manager.setup_page_resources(&mut resources);
        }

        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set(
            "MediaBox",
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(self.page_width as f32),
                Object::Real(self.page_height as f32),
            ]),
        );
        page.set(
            "CropBox",
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(self.page_width as f32),
                Object::Real(self.page_height as f32),
            ]),
        );
        page.set("Contents", Object::Reference(content_id));
        page.set("Resources", Object::Dictionary(resources));

        let page_id = self.pdf_doc.add_object(Object::Dictionary(page));

        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Count", Object::Integer(1));
        pages.set("Kids", Object::Array(vec![Object::Reference(page_id)]));

        let pages_id = self.pdf_doc.add_object(Object::Dictionary(pages));

        if let Ok(page_obj) = self.pdf_doc.get_object_mut(page_id) {
            if let Object::Dictionary(ref mut dict) = page_obj {
                dict.set("Parent", Object::Reference(pages_id));
            }
        }

        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference(pages_id));

        let catalog_id = self.pdf_doc.add_object(Object::Dictionary(catalog));
        self.pdf_doc.trailer.set("Root", Object::Reference(catalog_id));

        if self.options.export_layers && !layer_ocg_map.is_empty() {
            self.ocg_manager.update_catalog(&mut self.pdf_doc);
        }

        let mut info = Dictionary::new();
        if let Some(title) = &self.options.metadata.title {
            info.set("Title", Object::string_literal(title.as_str()));
        }
        if let Some(author) = &self.options.metadata.author {
            info.set("Author", Object::string_literal(author.as_str()));
        }
        if let Some(subject) = &self.options.metadata.subject {
            info.set("Subject", Object::string_literal(subject.as_str()));
        }
        if let Some(creator) = &self.options.metadata.creator {
            info.set("Creator", Object::string_literal(creator.as_str()));
        }
        let info_id = self.pdf_doc.add_object(Object::Dictionary(info));
        self.pdf_doc.trailer.set("Info", Object::Reference(info_id));

        let mut output = Vec::new();
        self.pdf_doc
            .save_to(&mut output)
            .map_err(|e| ConversionError::PdfGeneration(format!("Save PDF error: {}", e)))?;

        Ok(output)
    }

    fn render_entity(
        &self,
        entity: &EntityType,
        transform: &Transform2D,
        stream: &mut String,
        visited_blocks: &mut HashSet<String>,
        depth: usize,
    ) {
        if depth > 32 {
            return;
        }

        let dyn_ent = entity.as_entity();
        if dyn_ent.is_invisible() {
            return;
        }

        let layer_name = dyn_ent.layer();
        let layer_color = self.layer_colors.get(layer_name);

        let entity_color = dyn_ent.color();
        let rgb = resolve_color(&entity_color, layer_color, None, self.default_draw_rgb);
        let final_rgb = match self.options.color_mode {
            ColorMode::Normal => rgb,
            ColorMode::Monochrome => (0, 0, 0),
            ColorMode::Grayscale => {
                let gray = (0.299 * rgb.0 as f64 + 0.587 * rgb.1 as f64 + 0.114 * rgb.2 as f64) as u8;
                (gray, gray, gray)
            }
            ColorMode::Invert => (255 - rgb.0, 255 - rgb.1, 255 - rgb.2),
        };
        let (r, g, b) = rgb_to_pdf(final_rgb);

        let line_weight_val = dyn_ent.line_weight().millimeters().unwrap_or(0.25);
        let line_width_pt = (line_weight_val * 2.83465 * self.options.line_width_scale)
            .max(self.options.min_line_width_pt);

        stream.push_str(&format!(
            "{:.3} {:.3} {:.3} RG {:.3} {:.3} {:.3} rg {:.2} w\n",
            r, g, b, r, g, b, line_width_pt
        ));

        match entity {
            EntityType::Line(line) => {
                let (x1_cad, y1_cad) = transform.apply(line.start.x, line.start.y);
                let (x2_cad, y2_cad) = transform.apply(line.end.x, line.end.y);
                let (p1x, p1y) = self.transform_point(x1_cad, y1_cad);
                let (p2x, p2y) = self.transform_point(x2_cad, y2_cad);

                stream.push_str(&format!("{:.4} {:.4} m {:.4} {:.4} l S\n", p1x, p1y, p2x, p2y));
            }
            EntityType::Circle(circle) => {
                let (cx_cad, cy_cad) = transform.apply(circle.center.x, circle.center.y);
                let (cx, cy) = self.transform_point(cx_cad, cy_cad);
                let r = circle.radius * self.scale * transform.a.hypot(transform.b);

                stream.push_str(&generate_circle_path(cx, cy, r));
                stream.push_str("S\n");
            }
            EntityType::Arc(arc) => {
                let (cx_cad, cy_cad) = transform.apply(arc.center.x, arc.center.y);
                let (cx, cy) = self.transform_point(cx_cad, cy_cad);
                let r = arc.radius * self.scale * transform.a.hypot(transform.b);
                let start_rad = arc.start_angle.to_radians();
                let end_rad = arc.end_angle.to_radians();

                stream.push_str(&generate_arc_path(cx, cy, r, start_rad, end_rad));
                stream.push_str("S\n");
            }
            EntityType::Ellipse(ellipse) => {
                let (cx_cad, cy_cad) = transform.apply(ellipse.center.x, ellipse.center.y);
                let (cx, cy) = self.transform_point(cx_cad, cy_cad);

                let major_x = ellipse.major_axis.x * self.scale;
                let major_y = ellipse.major_axis.y * self.scale;

                stream.push_str(&generate_ellipse_path(
                    cx,
                    cy,
                    major_x,
                    major_y,
                    ellipse.minor_axis_ratio,
                    ellipse.start_parameter,
                    ellipse.end_parameter,
                ));
                stream.push_str("S\n");
            }
            EntityType::LwPolyline(lwpoly) => {
                if lwpoly.vertices.is_empty() {
                    return;
                }

                let first = &lwpoly.vertices[0];
                let (f_cad_x, f_cad_y) = transform.apply(first.location.x, first.location.y);
                let (p0_x, p0_y) = self.transform_point(f_cad_x, f_cad_y);
                stream.push_str(&format!("{:.4} {:.4} m\n", p0_x, p0_y));

                for i in 0..lwpoly.vertices.len() - 1 {
                    let v1 = &lwpoly.vertices[i];
                    let v2 = &lwpoly.vertices[i + 1];

                    let (v1_cx, v1_cy) = transform.apply(v1.location.x, v1.location.y);
                    let (v2_cx, v2_cy) = transform.apply(v2.location.x, v2.location.y);
                    let (p1x, p1y) = self.transform_point(v1_cx, v1_cy);
                    let (p2x, p2y) = self.transform_point(v2_cx, v2_cy);

                    if v1.bulge.abs() > 1e-6 {
                        stream.push_str(&generate_bulge_segment(
                            Point2D::new(p1x, p1y),
                            Point2D::new(p2x, p2y),
                            v1.bulge,
                        ));
                    } else {
                        stream.push_str(&format!("{:.4} {:.4} l\n", p2x, p2y));
                    }
                }

                if lwpoly.is_closed && lwpoly.vertices.len() > 1 {
                    let last = &lwpoly.vertices[lwpoly.vertices.len() - 1];
                    let first = &lwpoly.vertices[0];

                    let (v1_cx, v1_cy) = transform.apply(last.location.x, last.location.y);
                    let (v2_cx, v2_cy) = transform.apply(first.location.x, first.location.y);
                    let (p1x, p1y) = self.transform_point(v1_cx, v1_cy);
                    let (p2x, p2y) = self.transform_point(v2_cx, v2_cy);

                    if last.bulge.abs() > 1e-6 {
                        stream.push_str(&generate_bulge_segment(
                            Point2D::new(p1x, p1y),
                            Point2D::new(p2x, p2y),
                            last.bulge,
                        ));
                    }
                    stream.push_str("h ");
                }

                stream.push_str("S\n");
            }
            EntityType::Polyline(poly) => {
                if poly.vertices.is_empty() {
                    return;
                }
                let first = &poly.vertices[0];
                let (f_cad_x, f_cad_y) = transform.apply(first.location.x, first.location.y);
                let (p0_x, p0_y) = self.transform_point(f_cad_x, f_cad_y);
                stream.push_str(&format!("{:.4} {:.4} m\n", p0_x, p0_y));

                for i in 1..poly.vertices.len() {
                    let v = &poly.vertices[i];
                    let (v_cx, v_cy) = transform.apply(v.location.x, v.location.y);
                    let (px, py) = self.transform_point(v_cx, v_cy);
                    stream.push_str(&format!("{:.4} {:.4} l\n", px, py));
                }

                if poly.is_closed() {
                    stream.push_str("h ");
                }
                stream.push_str("S\n");
            }
            EntityType::Polyline2D(poly) => {
                if poly.vertices.is_empty() {
                    return;
                }
                let first = &poly.vertices[0];
                let (f_cad_x, f_cad_y) = transform.apply(first.location.x, first.location.y);
                let (p0_x, p0_y) = self.transform_point(f_cad_x, f_cad_y);
                stream.push_str(&format!("{:.4} {:.4} m\n", p0_x, p0_y));

                for i in 1..poly.vertices.len() {
                    let v = &poly.vertices[i];
                    let (v_cx, v_cy) = transform.apply(v.location.x, v.location.y);
                    let (px, py) = self.transform_point(v_cx, v_cy);
                    stream.push_str(&format!("{:.4} {:.4} l\n", px, py));
                }

                if poly.is_closed() {
                    stream.push_str("h ");
                }
                stream.push_str("S\n");
            }
            EntityType::Point(pt) => {
                let (px_cad, py_cad) = transform.apply(pt.location.x, pt.location.y);
                let (px, py) = self.transform_point(px_cad, py_cad);
                let sz = 2.0;
                stream.push_str(&format!(
                    "{:.4} {:.4} m {:.4} {:.4} l {:.4} {:.4} m {:.4} {:.4} l S\n",
                    px - sz, py, px + sz, py,
                    px, py - sz, px, py + sz
                ));
            }
            EntityType::Solid(solid) => {
                let (x1, y1) = self.transform_point(solid.first_corner.x, solid.first_corner.y);
                let (x2, y2) = self.transform_point(solid.second_corner.x, solid.second_corner.y);
                let (x3, y3) = self.transform_point(solid.third_corner.x, solid.third_corner.y);
                let (x4, y4) = self.transform_point(solid.fourth_corner.x, solid.fourth_corner.y);

                stream.push_str(&format!(
                    "{:.4} {:.4} m {:.4} {:.4} l {:.4} {:.4} l {:.4} {:.4} l h f\n",
                    x1, y1, x2, y2, x4, y4, x3, y3
                ));
            }
            EntityType::Face3D(face) => {
                let (x1, y1) = self.transform_point(face.first_corner.x, face.first_corner.y);
                let (x2, y2) = self.transform_point(face.second_corner.x, face.second_corner.y);
                let (x3, y3) = self.transform_point(face.third_corner.x, face.third_corner.y);
                let (x4, y4) = self.transform_point(face.fourth_corner.x, face.fourth_corner.y);

                stream.push_str(&format!(
                    "{:.4} {:.4} m {:.4} {:.4} l {:.4} {:.4} l {:.4} {:.4} l h S\n",
                    x1, y1, x2, y2, x3, y3, x4, y4
                ));
            }
            EntityType::Text(txt) => {
                let (x_cad, y_cad) = transform.apply(
                    txt.insertion_point.x,
                    txt.insertion_point.y,
                );
                let (px, py) = self.transform_point(x_cad, y_cad);
                let font_size = (txt.height * self.scale).max(4.0);

                let decoded = decode_cad_text_escapes(&txt.value);
                let escaped = escape_pdf_string(&decoded);

                let rot_rad = txt.rotation.to_radians();
                let cos = rot_rad.cos();
                let sin = rot_rad.sin();

                stream.push_str(&format!(
                    "BT\n\
                     /{} {:.2} Tf\n\
                     {:.4} {:.4} {:.4} {:.4} {:.4} {:.4} Tm\n\
                     ({}) Tj\n\
                     ET\n",
                    self.primary_font_name, font_size,
                    cos, sin, -sin, cos, px, py,
                    escaped
                ));
            }
            EntityType::MText(mtext) => {
                let (x_cad, y_cad) = transform.apply(
                    mtext.insertion_point.x,
                    mtext.insertion_point.y,
                );
                let (px, py) = self.transform_point(x_cad, y_cad);
                let font_size = (mtext.height * self.scale).max(4.0);

                let lines = clean_mtext(&mtext.value);
                let rot_rad = mtext.rotation.to_radians();
                let cos = rot_rad.cos();
                let sin = rot_rad.sin();

                for (idx, line) in lines.iter().enumerate() {
                    let escaped = escape_pdf_string(line);
                    let line_offset = (idx as f64) * font_size * 1.3;
                    let line_px = px + sin * line_offset;
                    let line_py = py - cos * line_offset;

                    stream.push_str(&format!(
                        "BT\n\
                         /{} {:.2} Tf\n\
                         {:.4} {:.4} {:.4} {:.4} {:.4} {:.4} Tm\n\
                         ({}) Tj\n\
                         ET\n",
                        self.primary_font_name, font_size,
                        cos, sin, -sin, cos, line_px, line_py,
                        escaped
                    ));
                }
            }
            EntityType::Spline(spline) => {
                if spline.control_points.len() >= 2 {
                    let first = &spline.control_points[0];
                    let (fx, fy) = transform.apply(first.x, first.y);
                    let (p0x, p0y) = self.transform_point(fx, fy);
                    stream.push_str(&format!("{:.4} {:.4} m\n", p0x, p0y));

                    for pt in &spline.control_points[1..] {
                        let (cx, cy) = transform.apply(pt.x, pt.y);
                        let (px, py) = self.transform_point(cx, cy);
                        stream.push_str(&format!("{:.4} {:.4} l\n", px, py));
                    }
                    if spline.flags.closed {
                        stream.push_str("h ");
                    }
                    stream.push_str("S\n");
                }
            }
            EntityType::Insert(insert) => {
                let insert_transform = Transform2D::from_trs(
                    insert.insert_point.x,
                    insert.insert_point.y,
                    insert.rotation,
                    insert.x_scale(),
                    insert.y_scale(),
                );
                let combined = transform.combine(&insert_transform);

                let block_name = &insert.block_name;
                if visited_blocks.insert(block_name.clone()) {
                    for block_ent in self.doc.entities_in_block(block_name) {
                        self.render_entity(block_ent, &combined, stream, visited_blocks, depth + 1);
                    }
                    visited_blocks.remove(block_name);
                }
            }
            EntityType::Dimension(dim) => {
                let block_name = &dim.base().block_name;
                if !block_name.is_empty() && visited_blocks.insert(block_name.clone()) {
                    for block_ent in self.doc.entities_in_block(block_name) {
                        self.render_entity(block_ent, transform, stream, visited_blocks, depth + 1);
                    }
                    visited_blocks.remove(block_name);
                }
            }
            _ => {}
        }
    }
}

fn escape_pdf_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '(' => out.push_str(r"\("),
            ')' => out.push_str(r"\)"),
            '\\' => out.push_str(r"\\"),
            '\r' => out.push_str(r"\r"),
            '\n' => out.push_str(r"\n"),
            '\t' => out.push_str(r"\t"),
            _ => out.push(ch),
        }
    }
    out
}
