use crate::cad::bounds::{calculate_block_bounds, calculate_document_bounds, entity_intersects_bbox, BoundingBox, Transform2D};
use crate::cad::color::{parse_hex_color, resolve_color, rgb_to_pdf};
use crate::cad::mtext::{decode_cad_text_escapes, estimate_str_width, parse_mtext_details, MTextAlignment};
use crate::error::{ConversionError, ConversionResult};
use crate::options::{ColorMode, ConversionMode, ConversionOptions, Orientation, PageSize};
use crate::pdf::geometry::{
    generate_arc_path, generate_bulge_segment, generate_circle_path, generate_ellipse_path, Point2D,
};

use acadrust::document::CadDocument;
use acadrust::entities::{BoundaryEdge, EntityType};
use acadrust::objects::ObjectType;
use acadrust::types::color::Color;

use hipdf::fonts::{Font, FontManager, StandardFont};
use hipdf::lopdf::{Dictionary, Document, Object, Stream};
use hipdf::ocg::OCGManager;

use std::collections::{HashMap, HashSet};

#[derive(Default, Clone, Copy)]
pub struct GraphicsState {
    pub stroke_rgb: Option<(f32, f32, f32)>,
    pub fill_rgb: Option<(f32, f32, f32)>,
    pub line_width: Option<f64>,
}

pub struct CadToPdfBuilder<'a> {
    doc: &'a CadDocument,
    options: ConversionOptions,
    bbox: BoundingBox,
    pdf_doc: Document,
    ocg_manager: OCGManager,
    layer_ocg_map: HashMap<String, String>,
    font_manager: FontManager,
    primary_font_id: (u32, u16),
    primary_font_name: String,
    bold_font_id: (u32, u16),
    bold_font_name: String,
    oblique_font_id: (u32, u16),
    oblique_font_name: String,
    bold_oblique_font_id: (u32, u16),
    bold_oblique_font_name: String,
    layer_colors: HashMap<String, Color>,
    default_draw_rgb: (u8, u8, u8),
    block_bounds_cache: HashMap<String, BoundingBox>,
}

impl<'a> CadToPdfBuilder<'a> {
    pub fn new(doc: &'a CadDocument, options: ConversionOptions) -> ConversionResult<Self> {
        let bbox = calculate_document_bounds(doc, true);

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

        let helvetica_bold = Font::standard(StandardFont::HelveticaBold);
        let (bold_font_id, bold_font_name) = font_manager
            .embed_font(&mut pdf_doc, helvetica_bold)
            .map_err(|e| ConversionError::PdfGeneration(format!("Font embedding error: {}", e)))?;

        let helvetica_oblique = Font::standard(StandardFont::HelveticaOblique);
        let (oblique_font_id, oblique_font_name) = font_manager
            .embed_font(&mut pdf_doc, helvetica_oblique)
            .map_err(|e| ConversionError::PdfGeneration(format!("Font embedding error: {}", e)))?;

        let helvetica_bold_oblique = Font::standard(StandardFont::HelveticaBoldOblique);
        let (bold_oblique_font_id, bold_oblique_font_name) = font_manager
            .embed_font(&mut pdf_doc, helvetica_bold_oblique)
            .map_err(|e| ConversionError::PdfGeneration(format!("Font embedding error: {}", e)))?;

        let mut ocg_manager = OCGManager::new();
        let mut layer_ocg_map = HashMap::new();

        if options.export_layers {
            for layer in doc.layers.iter() {
                if layer.name.eq_ignore_ascii_case("defpoints") || layer.name.to_lowercase().contains("nplt") || !layer.is_plottable {
                    continue;
                }
                let is_visible = !layer.is_frozen() && !layer.is_off();
                let idx = ocg_manager.add_layer(hipdf::ocg::Layer::new(&layer.name, is_visible));
                let tag = format!("L{}", idx);
                layer_ocg_map.insert(layer.name.clone(), tag);
            }
            if !layer_ocg_map.is_empty() {
                ocg_manager.initialize(&mut pdf_doc);
            }
        }

        Ok(Self {
            doc,
            options,
            bbox,
            pdf_doc,
            ocg_manager,
            layer_ocg_map,
            font_manager,
            primary_font_id,
            primary_font_name,
            bold_font_id,
            bold_font_name,
            oblique_font_id,
            oblique_font_name,
            bold_oblique_font_id,
            bold_oblique_font_name,
            layer_colors,
            default_draw_rgb,
            block_bounds_cache: HashMap::new(),
        })
    }

    fn select_font_name(&self, is_bold: bool, is_italic: bool) -> &str {
        match (is_bold, is_italic) {
            (true, true) => &self.bold_oblique_font_name,
            (true, false) => &self.bold_font_name,
            (false, true) => &self.oblique_font_name,
            (false, false) => &self.primary_font_name,
        }
    }

    fn resolve_mtext_value<'b>(&'b self, mtext: &'b acadrust::entities::MText) -> std::borrow::Cow<'b, str> {
        if let Some(xdict_handle) = mtext.common.xdictionary_handle {
            if let Some(ObjectType::Dictionary(dict)) = self.doc.objects.get(&xdict_handle) {
                if let Some((_, xrec_handle)) = dict.entries.iter().find(|(k, _)| k.eq_ignore_ascii_case("ACAD_MTEXT_2008_RT")) {
                    if let Some(ObjectType::XRecord(xrec)) = self.doc.objects.get(xrec_handle) {
                        let mut full_text = String::new();
                        for entry in &xrec.entries {
                            if entry.code == 3 || entry.code == 1 {
                                if let Some(s) = entry.value.as_string() {
                                    full_text.push_str(s);
                                }
                            }
                        }
                        if !full_text.is_empty() {
                            return std::borrow::Cow::Owned(full_text);
                        }
                    }
                }
            }
        }
        std::borrow::Cow::Borrowed(&mtext.value)
    }

    pub fn bbox(&self) -> &BoundingBox {
        &self.bbox
    }

    fn compute_model_page_and_transform(
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

                // If drawing dimensions in CAD units exceed standard large format sheet (A0: 3370.39 pt),
                // scale page down proportionally so maximum dimension does not exceed 3370.39 pt (ISO 216 A0).
                let max_dim = w.max(h);
                if max_dim > 3370.39 {
                    let shrink = 3370.39 / max_dim;
                    w *= shrink;
                    h *= shrink;
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

    /// Resolves the backing block name for a paper space layout.
    fn find_layout_block_name(&self, layout: &acadrust::objects::Layout) -> Option<String> {
        // 1. By block_record handle or layout handle
        if let Some(b) = self.doc.block_records.iter().find(|b| {
            (layout.block_record.value() != 0 && b.handle == layout.block_record)
                || (layout.handle.value() != 0 && b.layout == layout.handle)
        }) {
            return Some(b.name.clone());
        }

        // 2. By layout name directly
        if let Some(b) = self.doc.block_records.iter().find(|b| b.name.eq_ignore_ascii_case(&layout.name)) {
            return Some(b.name.clone());
        }

        // 3. By standard AutoCAD paper space naming convention
        let candidate_names = [
            format!("*Paper_Space{}", if layout.tab_order <= 1 { String::new() } else { (layout.tab_order - 1).to_string() }),
            format!("*Paper_Space{}", layout.tab_order),
            "*Paper_Space".to_string(),
        ];
        for name in &candidate_names {
            if self.doc.block_records.iter().any(|b| b.name.eq_ignore_ascii_case(name)) {
                return Some(name.clone());
            }
        }

        None
    }

    /// Checks whether a layout is enabled and visible (contains sheet geometry or active model viewports).
    fn is_layout_visible_and_enabled(&self, layout: &acadrust::objects::Layout) -> bool {
        // Model space is handled separately as the final page
        if layout.name.eq_ignore_ascii_case("model") || layout.tab_order <= 0 {
            return false;
        }

        let block_name = match self.find_layout_block_name(layout) {
            Some(name) => name,
            None => return false,
        };

        let mut has_sheet_entities = false;
        let mut has_active_viewport = false;

        for entity in self.doc.entities_in_block(&block_name) {
            match entity {
                EntityType::Viewport(vp) => {
                    let is_overall = vp.id == 1 || (
                        (vp.center.x - vp.view_center.x).abs() < 1e-3
                            && (vp.center.y - vp.view_center.y).abs() < 1e-3
                            && (vp.height - vp.view_height).abs() < 1e-3
                    );
                    if !is_overall && vp.status.is_on && vp.width > 0.0 && vp.height > 0.0 && vp.view_height > 0.0 {
                        has_active_viewport = true;
                    }
                }
                _ => {
                    has_sheet_entities = true;
                }
            }
        }

        // A layout is enabled and visible if it contains sheet entities (title block, annotations, geometry)
        // or active viewports looking into model space.
        has_sheet_entities || has_active_viewport
    }

    pub fn build(mut self) -> ConversionResult<Vec<u8>> {
        let mut page_ids = Vec::new();

        // Check if a specific layout was requested
        let specific_layout = self.options.layout.clone();

        if let Some(target_layout) = specific_layout {
            if target_layout.eq_ignore_ascii_case("model") {
                let page_id = self.build_model_space_page()?;
                page_ids.push(page_id);
            } else {
                // Find target paper space layout
                let layout_obj = self.doc.objects.iter().find_map(|(_h, obj)| {
                    if let ObjectType::Layout(l) = obj {
                        if l.name.eq_ignore_ascii_case(&target_layout) {
                            return Some(l.clone());
                        }
                    }
                    None
                });

                if let Some(l) = layout_obj {
                    let page_id = self.build_paper_space_page(&l)?;
                    page_ids.push(page_id);
                } else {
                    // Fall back to model space if not found
                    let page_id = self.build_model_space_page()?;
                    page_ids.push(page_id);
                }
            }
        } else {
            // Default multi-page mode:
            // 1. Each enabled and visible paper space layout is a page
            let mut paper_layouts: Vec<_> = self.doc.objects.iter().filter_map(|(_h, obj)| {
                if let ObjectType::Layout(l) = obj {
                    if self.is_layout_visible_and_enabled(l) {
                        return Some(l.clone());
                    }
                }
                None
            }).collect();

            // Sort layouts strictly by their AutoCAD tab order (DXF group 71), with name as tie-breaker
            paper_layouts.sort_by(|a, b| {
                a.tab_order.cmp(&b.tab_order)
                    .then_with(|| a.name.cmp(&b.name))
            });

            for l in &paper_layouts {
                let page_id = self.build_paper_space_page(l)?;
                page_ids.push(page_id);
            }

            // 2. Model Space is ALWAYS the last page!
            let model_page_id = self.build_model_space_page()?;
            page_ids.push(model_page_id);
        }

        if page_ids.is_empty() {
            let model_page_id = self.build_model_space_page()?;
            page_ids.push(model_page_id);
        }

        // Create PDF Pages object
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Count", Object::Integer(page_ids.len() as i64));
        pages.set("Kids", Object::Array(page_ids.iter().map(|id| Object::Reference(*id)).collect()));

        let pages_id = self.pdf_doc.add_object(Object::Dictionary(pages));

        // Update each page's Parent reference
        for page_id in &page_ids {
            if let Ok(page_obj) = self.pdf_doc.get_object_mut(*page_id) {
                if let Object::Dictionary(ref mut dict) = page_obj {
                    dict.set("Parent", Object::Reference(pages_id));
                }
            }
        }

        // Create Catalog
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference(pages_id));

        let catalog_id = self.pdf_doc.add_object(Object::Dictionary(catalog));
        self.pdf_doc.trailer.set("Root", Object::Reference(catalog_id));

        // Update layers in catalog if layers exported
        if self.options.export_layers && !self.layer_ocg_map.is_empty() {
            self.ocg_manager.update_catalog(&mut self.pdf_doc);
        }

        // Metadata
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
        self.pdf_doc.compress();
        self.pdf_doc
            .save_to(&mut output)
            .map_err(|e| ConversionError::PdfGeneration(format!("Save PDF error: {}", e)))?;

        Ok(output)
    }

    /// Builds a single paper space layout page.
    fn build_paper_space_page(
        &mut self,
        layout: &acadrust::objects::Layout,
    ) -> ConversionResult<(u32, u16)> {
        // Find block name associated with this layout
        let block_name = self.find_layout_block_name(layout).unwrap_or_else(|| "*Paper_Space".to_string());

        // Determine paper size and coordinate transform
        let extent_w = (layout.max_limits.0 - layout.min_limits.0).abs();
        let extent_h = (layout.max_limits.1 - layout.min_limits.1).abs();

        let mm_to_pt = 72.0 / 25.4; // 2.83464567 pt/mm

        let (page_w_pt, page_h_pt, page_scale, page_tx, page_ty) = if layout.paper_width > 10.0 && layout.paper_height > 10.0 {
            // Known paper dimensions in mm
            let p_w = layout.paper_width;
            let p_h = layout.paper_height;

            // Check orientation from extent or rotation
            let is_landscape = if extent_w > 0.1 && extent_h > 0.1 {
                extent_w > extent_h
            } else {
                layout.plot_rotation == 1 || layout.plot_rotation == 3 || p_w > p_h
            };

            let (sheet_w_mm, sheet_h_mm) = if is_landscape {
                (p_w.max(p_h), p_w.min(p_h))
            } else {
                (p_w.min(p_h), p_w.max(p_h))
            };

            let sheet_w_pt = sheet_w_mm * mm_to_pt;
            let sheet_h_pt = sheet_h_mm * mm_to_pt;

            let (scale, tx, ty) = if extent_w > 0.01 && extent_h > 0.01 {
                let s = (sheet_w_pt / extent_w).min(sheet_h_pt / extent_h);
                let min_x = layout.min_limits.0.min(layout.max_limits.0);
                let min_y = layout.min_limits.1.min(layout.max_limits.1);
                let t_x = (sheet_w_pt - extent_w * s) / 2.0 - min_x * s;
                let t_y = (sheet_h_pt - extent_h * s) / 2.0 - min_y * s;
                (s, t_x, t_y)
            } else {
                let block_bounds = calculate_block_bounds(self.doc, &block_name);
                if block_bounds.is_valid() {
                    let bw = block_bounds.width().max(1.0);
                    let bh = block_bounds.height().max(1.0);
                    let s = (sheet_w_pt / bw).min(sheet_h_pt / bh);
                    let t_x = (sheet_w_pt - bw * s) / 2.0 - block_bounds.min_x * s;
                    let t_y = (sheet_h_pt - bh * s) / 2.0 - block_bounds.min_y * s;
                    (s, t_x, t_y)
                } else {
                    (mm_to_pt, 0.0, 0.0)
                }
            };

            (sheet_w_pt, sheet_h_pt, scale, tx, ty)
        } else {
            // Paper dimensions not explicitly set; compute from block bounds
            let block_bounds = calculate_block_bounds(self.doc, &block_name);
            let margin = 20.0;
            let bw = block_bounds.width().max(100.0);
            let bh = block_bounds.height().max(100.0);
            let pw = bw + 2.0 * margin;
            let ph = bh + 2.0 * margin;
            let scale = 1.0;
            let tx = margin - block_bounds.min_x;
            let ty = margin - block_bounds.min_y;
            (pw, ph, scale, tx, ty)
        };

        let page_transform = Transform2D::from_scale_and_translation(page_scale, page_tx, page_ty);

        let mut page_stream = String::with_capacity(65536);

        // 1. Background color
        if let Some(bg_color_str) = &self.options.background_color {
            if let Some(rgb) = parse_hex_color(bg_color_str) {
                let (r, g, b) = rgb_to_pdf(rgb);
                page_stream.push_str(&format!(
                    "q\n{:.3} {:.3} {:.3} rg\n0 0 {:.2} {:.2} re\nf\nQ\n",
                    r, g, b, page_w_pt, page_h_pt
                ));
            }
        }

        // Set default graphics state
        page_stream.push_str("q\n1 J\n1 j\n");

        // Separate layout entities into viewports and sheet elements
        let mut viewports = Vec::new();
        let mut sheet_entities: Vec<&EntityType> = Vec::new();

        for entity in self.doc.entities_in_block(&block_name) {
            if let EntityType::Viewport(vp) = entity {
                viewports.push(vp);
            } else {
                sheet_entities.push(entity);
            }
        }

        // Group model space entities by layer ONCE outside the viewports loop
        let mut model_ents_by_layer: HashMap<String, Vec<&EntityType>> = HashMap::new();
        for entity in self.doc.model_space_entities() {
            let layer_name = entity.as_entity().layer().to_string();
            model_ents_by_layer.entry(layer_name).or_default().push(entity);
        }

        // 2. Render each Viewport looking into Model Space
        for vp in viewports {
            let is_overall = (vp.center.x - vp.view_center.x).abs() < 1e-3
                && (vp.center.y - vp.view_center.y).abs() < 1e-3
                && (vp.height - vp.view_height).abs() < 1e-3;

            if is_overall || vp.id == 1 || !vp.status.is_on || vp.width <= 0.0 || vp.height <= 0.0 || vp.view_height <= 0.0 {
                continue;
            }

            let (vp_cx, vp_cy) = page_transform.apply(vp.center.x, vp.center.y);
            let vp_w = vp.width * page_scale;
            let vp_h = vp.height * page_scale;
            let clip_x = vp_cx - vp_w / 2.0;
            let clip_y = vp_cy - vp_h / 2.0;

            let vp_scale = vp_h / vp.view_height;
            let twist_rad = -vp.twist_angle;
            let cos_tw = twist_rad.cos();
            let sin_tw = twist_rad.sin();

            // In AutoCAD DCS, the view center is relative to view_target in WCS
            let model_cx = vp.view_target.x + vp.view_center.x * cos_tw - vp.view_center.y * sin_tw;
            let model_cy = vp.view_target.y + vp.view_center.x * sin_tw + vp.view_center.y * cos_tw;

            let a = cos_tw * vp_scale;
            let b = sin_tw * vp_scale;
            let c = -sin_tw * vp_scale;
            let d = cos_tw * vp_scale;

            let vp_transform = Transform2D {
                a,
                b,
                c,
                d,
                tx: vp_cx - (a * model_cx + c * model_cy),
                ty: vp_cy - (b * model_cx + d * model_cy),
            };

            let frozen_handles: HashSet<_> = vp.frozen_layers.iter().copied().collect();

            // Calculate Model Space bounding box for this viewport
            let vp_aspect = if vp.height > 0.0 { vp.width / vp.height } else { 1.0 };
            let view_w = vp.view_height * vp_aspect;
            let view_h = vp.view_height;
            let cos_abs = cos_tw.abs();
            let sin_abs = sin_tw.abs();
            let extent_x = (view_w * cos_abs + view_h * sin_abs) / 2.0;
            let extent_y = (view_w * sin_abs + view_h * cos_abs) / 2.0;
            let mx = (extent_x * 0.15).max(10.0);
            let my = (extent_y * 0.15).max(10.0);
            let vp_model_box = BoundingBox {
                min_x: model_cx - extent_x - mx,
                min_y: model_cy - extent_y - my,
                max_x: model_cx + extent_x + mx,
                max_y: model_cy + extent_y + my,
            };

            // Push clipping boundary
            let mut vp_stream = format!(
                "q\n{:.2} {:.2} {:.2} {:.2} re W n\n",
                clip_x, clip_y, vp_w, vp_h
            );

            let mut has_vp_entities = false;

            for (layer_name, layer_ents) in &model_ents_by_layer {
                if layer_name.eq_ignore_ascii_case("defpoints") {
                    continue;
                }
                if let Some(layer) = self.doc.layers.iter().find(|l| l.name == *layer_name) {
                    if !layer.is_plottable || frozen_handles.contains(&layer.handle) {
                        continue;
                    }
                    if !self.options.export_layers && (layer.is_frozen() || layer.is_off()) {
                        continue;
                    }
                }

                let mut layer_stream = String::new();
                let mut visited_blocks = HashSet::new();
                let mut gstate = GraphicsState::default();

                for entity in layer_ents {
                    // Spatial culling: skip entities outside the viewport model box
                    if !entity_intersects_bbox(self.doc, entity, &vp_model_box, &mut self.block_bounds_cache) {
                        continue;
                    }

                    self.render_entity(
                        entity,
                        &vp_transform,
                        &mut layer_stream,
                        &mut gstate,
                        &mut visited_blocks,
                        0,
                    );
                }

                if layer_stream.is_empty() {
                    continue;
                }

                has_vp_entities = true;

                if self.options.export_layers {
                    if let Some(prop_tag) = self.layer_ocg_map.get(layer_name) {
                        vp_stream.push_str(&format!("/OC /{} BDC\n", prop_tag));
                        vp_stream.push_str(&layer_stream);
                        vp_stream.push_str("EMC\n");
                    } else {
                        vp_stream.push_str(&layer_stream);
                    }
                } else {
                    vp_stream.push_str(&layer_stream);
                }
            }

            vp_stream.push_str("Q\n");

            if has_vp_entities {
                page_stream.push_str(&vp_stream);
            }
        }

        // 3. Render sheet entities on top of viewports (title blocks, borders, text)
        let mut sheet_ents_by_layer: HashMap<String, Vec<&EntityType>> = HashMap::new();
        for entity in sheet_entities {
            let layer_name = entity.as_entity().layer().to_string();
            sheet_ents_by_layer.entry(layer_name).or_default().push(entity);
        }

        for (layer_name, layer_ents) in sheet_ents_by_layer {
            if layer_name.eq_ignore_ascii_case("defpoints") {
                continue;
            }
            if let Some(layer) = self.doc.layers.iter().find(|l| l.name == layer_name) {
                if !layer.is_plottable {
                    continue;
                }
                if !self.options.export_layers && (layer.is_frozen() || layer.is_off()) {
                    continue;
                }
            }

            let mut layer_stream = String::new();
            let mut visited_blocks = HashSet::new();
            let mut gstate = GraphicsState::default();

            for entity in layer_ents {
                self.render_entity(
                    entity,
                    &page_transform,
                    &mut layer_stream,
                    &mut gstate,
                    &mut visited_blocks,
                    0,
                );
            }

            if layer_stream.is_empty() {
                continue;
            }

            if self.options.export_layers {
                if let Some(prop_tag) = self.layer_ocg_map.get(&layer_name) {
                    page_stream.push_str(&format!("/OC /{} BDC\n", prop_tag));
                    page_stream.push_str(&layer_stream);
                    page_stream.push_str("EMC\n");
                } else {
                    page_stream.push_str(&layer_stream);
                }
            } else {
                page_stream.push_str(&layer_stream);
            }
        }

        page_stream.push_str("Q\n");

        let stream = Stream::new(Dictionary::new(), page_stream.into_bytes());
        let content_id = self.pdf_doc.add_object(Object::Stream(stream));

        let mut resources = Dictionary::new();
        self.font_manager.add_to_resources(&mut resources, self.primary_font_id, &self.primary_font_name);
        self.font_manager.add_to_resources(&mut resources, self.bold_font_id, &self.bold_font_name);
        self.font_manager.add_to_resources(&mut resources, self.oblique_font_id, &self.oblique_font_name);
        self.font_manager.add_to_resources(&mut resources, self.bold_oblique_font_id, &self.bold_oblique_font_name);

        if self.options.export_layers && !self.layer_ocg_map.is_empty() {
            self.ocg_manager.setup_page_resources(&mut resources);
        }

        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set(
            "MediaBox",
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(page_w_pt as f32),
                Object::Real(page_h_pt as f32),
            ]),
        );
        page.set(
            "CropBox",
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(page_w_pt as f32),
                Object::Real(page_h_pt as f32),
            ]),
        );
        page.set("Contents", Object::Reference(content_id));
        page.set("Resources", Object::Dictionary(resources));

        let page_id = self.pdf_doc.add_object(Object::Dictionary(page));
        Ok(page_id)
    }

    /// Builds the Model Space page (always the last page).
    fn build_model_space_page(&mut self) -> ConversionResult<(u32, u16)> {
        let bbox = match &self.options.mode {
            ConversionMode::Plot => self.bbox,
            ConversionMode::Crop {
                offset_x,
                offset_y,
                width,
                height,
            } => {
                let w = width.unwrap_or(self.bbox.width());
                let h = height.unwrap_or(self.bbox.height());
                BoundingBox {
                    min_x: *offset_x,
                    min_y: *offset_y,
                    max_x: *offset_x + w,
                    max_y: *offset_y + h,
                }
            }
        };

        let margin = self.options.margin_pt;
        let (page_w, page_h, scale, tx, ty) = Self::compute_model_page_and_transform(&bbox, &self.options, margin);
        let page_transform = Transform2D::from_scale_and_translation(scale, tx, ty);

        let mut page_stream = String::with_capacity(65536);

        // Background color
        if let Some(bg_color_str) = &self.options.background_color {
            if let Some(rgb) = parse_hex_color(bg_color_str) {
                let (r, g, b) = rgb_to_pdf(rgb);
                page_stream.push_str(&format!(
                    "q\n{:.3} {:.3} {:.3} rg\n0 0 {:.2} {:.2} re\nf\nQ\n",
                    r, g, b, page_w, page_h
                ));
            }
        }

        page_stream.push_str("q\n1 J\n1 j\n");

        let mut entities_by_layer: HashMap<String, Vec<&EntityType>> = HashMap::new();
        for entity in self.doc.model_space_entities() {
            let layer_name = entity.as_entity().layer().to_string();
            entities_by_layer.entry(layer_name).or_default().push(entity);
        }

        for (layer_name, layer_entities) in entities_by_layer {
            if layer_name.eq_ignore_ascii_case("defpoints") {
                continue;
            }
            if let Some(layer) = self.doc.layers.iter().find(|l| l.name == layer_name) {
                if !layer.is_plottable {
                    continue;
                }
                if !self.options.export_layers && (layer.is_frozen() || layer.is_off()) {
                    continue;
                }
            }

            let mut layer_stream = String::new();
            let mut visited_blocks = HashSet::new();
            let mut gstate = GraphicsState::default();

            for entity in layer_entities {
                self.render_entity(
                    entity,
                    &page_transform,
                    &mut layer_stream,
                    &mut gstate,
                    &mut visited_blocks,
                    0,
                );
            }

            if layer_stream.is_empty() {
                continue;
            }

            if self.options.export_layers {
                if let Some(prop_tag) = self.layer_ocg_map.get(&layer_name) {
                    page_stream.push_str(&format!("/OC /{} BDC\n", prop_tag));
                    page_stream.push_str(&layer_stream);
                    page_stream.push_str("EMC\n");
                } else {
                    page_stream.push_str(&layer_stream);
                }
            } else {
                page_stream.push_str(&layer_stream);
            }
        }

        page_stream.push_str("Q\n");

        let stream = Stream::new(Dictionary::new(), page_stream.into_bytes());
        let content_id = self.pdf_doc.add_object(Object::Stream(stream));

        let mut resources = Dictionary::new();
        self.font_manager.add_to_resources(&mut resources, self.primary_font_id, &self.primary_font_name);
        self.font_manager.add_to_resources(&mut resources, self.bold_font_id, &self.bold_font_name);
        self.font_manager.add_to_resources(&mut resources, self.oblique_font_id, &self.oblique_font_name);
        self.font_manager.add_to_resources(&mut resources, self.bold_oblique_font_id, &self.bold_oblique_font_name);

        if self.options.export_layers && !self.layer_ocg_map.is_empty() {
            self.ocg_manager.setup_page_resources(&mut resources);
        }

        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set(
            "MediaBox",
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(page_w as f32),
                Object::Real(page_h as f32),
            ]),
        );
        page.set(
            "CropBox",
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(page_w as f32),
                Object::Real(page_h as f32),
            ]),
        );
        page.set("Contents", Object::Reference(content_id));
        page.set("Resources", Object::Dictionary(resources));

        let page_id = self.pdf_doc.add_object(Object::Dictionary(page));
        Ok(page_id)
    }

    #[inline]
    fn ensure_stroke(stream: &mut String, state: &mut GraphicsState, r: f32, g: f32, b: f32, line_width_pt: f64) {
        if state.stroke_rgb != Some((r, g, b)) {
            stream.push_str(&format!("{:.3} {:.3} {:.3} RG\n", r, g, b));
            state.stroke_rgb = Some((r, g, b));
        }
        if state.line_width.map_or(true, |cur| (cur - line_width_pt).abs() > 1e-4) {
            if line_width_pt <= 1e-4 {
                stream.push_str("0 w\n");
            } else {
                stream.push_str(&format!("{:.2} w\n", line_width_pt));
            }
            state.line_width = Some(line_width_pt);
        }
    }

    #[inline]
    fn ensure_fill(stream: &mut String, state: &mut GraphicsState, r: f32, g: f32, b: f32) {
        if state.fill_rgb != Some((r, g, b)) {
            stream.push_str(&format!("{:.3} {:.3} {:.3} rg\n", r, g, b));
            state.fill_rgb = Some((r, g, b));
        }
    }

    fn render_entity(
        &mut self,
        entity: &EntityType,
        transform: &Transform2D,
        stream: &mut String,
        state: &mut GraphicsState,
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
        if layer_name.eq_ignore_ascii_case("defpoints") {
            return;
        }

        if let Some(layer) = self.doc.layers.iter().find(|l| l.name == layer_name) {
            if !layer.is_plottable {
                return;
            }
            if !self.options.export_layers && (layer.is_frozen() || layer.is_off()) {
                return;
            }
        }

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

        let line_width_pt = if self.options.use_lineweights {
            let line_weight_val = dyn_ent.line_weight().millimeters().unwrap_or(0.15);
            (line_weight_val * 2.83465 * self.options.line_width_scale).max(self.options.min_line_width_pt)
        } else {
            (self.options.min_line_width_pt * self.options.line_width_scale).max(0.0)
        };

        match entity {
            EntityType::Line(line) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                let (p1x, p1y) = transform.apply(line.start.x, line.start.y);
                let (p2x, p2y) = transform.apply(line.end.x, line.end.y);
                stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", p1x, p1y, p2x, p2y));
            }
            EntityType::Circle(circle) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                let (cx, cy) = transform.apply(circle.center.x, circle.center.y);
                let r_rad = circle.radius * transform.scale();
                stream.push_str(&generate_circle_path(cx, cy, r_rad));
                stream.push_str("S\n");
            }
            EntityType::Arc(arc) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                let (cx, cy) = transform.apply(arc.center.x, arc.center.y);
                let r_rad = arc.radius * transform.scale();
                let rot = transform.rotation_radians();
                let start_rad = arc.start_angle + rot;
                let end_rad = arc.end_angle + rot;
                stream.push_str(&generate_arc_path(cx, cy, r_rad, start_rad, end_rad));
                stream.push_str("S\n");
            }
            EntityType::Ellipse(ellipse) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                let (cx, cy) = transform.apply(ellipse.center.x, ellipse.center.y);
                let maj_x = ellipse.major_axis.x * transform.a + ellipse.major_axis.y * transform.c;
                let maj_y = ellipse.major_axis.x * transform.b + ellipse.major_axis.y * transform.d;
                stream.push_str(&generate_ellipse_path(
                    cx,
                    cy,
                    maj_x,
                    maj_y,
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
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                let first = &lwpoly.vertices[0];
                let (p0_x, p0_y) = transform.apply(first.location.x, first.location.y);
                stream.push_str(&format!("{:.2} {:.2} m\n", p0_x, p0_y));

                for i in 0..lwpoly.vertices.len() - 1 {
                    let v1 = &lwpoly.vertices[i];
                    let v2 = &lwpoly.vertices[i + 1];

                    let (p1x, p1y) = transform.apply(v1.location.x, v1.location.y);
                    let (p2x, p2y) = transform.apply(v2.location.x, v2.location.y);

                    if v1.bulge.abs() > 1e-6 {
                        stream.push_str(&generate_bulge_segment(
                            Point2D::new(p1x, p1y),
                            Point2D::new(p2x, p2y),
                            v1.bulge,
                        ));
                    } else {
                        stream.push_str(&format!("{:.2} {:.2} l\n", p2x, p2y));
                    }
                }

                if lwpoly.is_closed && lwpoly.vertices.len() > 1 {
                    let last = &lwpoly.vertices[lwpoly.vertices.len() - 1];
                    let first = &lwpoly.vertices[0];

                    let (p1x, p1y) = transform.apply(last.location.x, last.location.y);
                    let (p2x, p2y) = transform.apply(first.location.x, first.location.y);

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
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                let first = &poly.vertices[0];
                let (p0_x, p0_y) = transform.apply(first.location.x, first.location.y);
                stream.push_str(&format!("{:.2} {:.2} m\n", p0_x, p0_y));

                for i in 1..poly.vertices.len() {
                    let v = &poly.vertices[i];
                    let (px, py) = transform.apply(v.location.x, v.location.y);
                    stream.push_str(&format!("{:.2} {:.2} l\n", px, py));
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
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                let first = &poly.vertices[0];
                let (p0_x, p0_y) = transform.apply(first.location.x, first.location.y);
                stream.push_str(&format!("{:.2} {:.2} m\n", p0_x, p0_y));

                for i in 1..poly.vertices.len() {
                    let v = &poly.vertices[i];
                    let (px, py) = transform.apply(v.location.x, v.location.y);
                    stream.push_str(&format!("{:.2} {:.2} l\n", px, py));
                }

                if poly.is_closed() {
                    stream.push_str("h ");
                }
                stream.push_str("S\n");
            }
            EntityType::Point(pt) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                let (px, py) = transform.apply(pt.location.x, pt.location.y);
                let sz = (line_width_pt * 2.0).clamp(0.1, 0.5);
                stream.push_str(&format!(
                    "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} m {:.2} {:.2} l S\n",
                    px - sz, py, px + sz, py,
                    px, py - sz, px, py + sz
                ));
            }
            EntityType::Solid(solid) => {
                Self::ensure_fill(stream, state, r, g, b);
                let (x1, y1) = transform.apply(solid.first_corner.x, solid.first_corner.y);
                let (x2, y2) = transform.apply(solid.second_corner.x, solid.second_corner.y);
                let (x3, y3) = transform.apply(solid.third_corner.x, solid.third_corner.y);
                let (x4, y4) = transform.apply(solid.fourth_corner.x, solid.fourth_corner.y);

                stream.push_str(&format!(
                    "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h f\n",
                    x1, y1, x2, y2, x4, y4, x3, y3
                ));
            }
            EntityType::Face3D(face) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                let (x1, y1) = transform.apply(face.first_corner.x, face.first_corner.y);
                let (x2, y2) = transform.apply(face.second_corner.x, face.second_corner.y);
                let (x3, y3) = transform.apply(face.third_corner.x, face.third_corner.y);
                let (x4, y4) = transform.apply(face.fourth_corner.x, face.fourth_corner.y);

                stream.push_str(&format!(
                    "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h S\n",
                    x1, y1, x2, y2, x3, y3, x4, y4
                ));
            }
            EntityType::Text(txt) => {
                Self::ensure_fill(stream, state, r, g, b);
                let use_align_pt = txt.alignment_point.is_some() && (
                    txt.horizontal_alignment as u8 != 0 || txt.vertical_alignment as u8 != 0
                );
                let pt = if use_align_pt {
                    txt.alignment_point.as_ref().unwrap()
                } else {
                    &txt.insertion_point
                };
                let (px, py) = transform.apply(pt.x, pt.y);
                let font_size = (txt.height * transform.scale()).max(0.05);

                let is_bold = txt.style.to_lowercase().contains("bold") || self.doc.text_styles.iter().any(|s| {
                    s.name.eq_ignore_ascii_case(&txt.style) && (s.name.to_lowercase().contains("bold") || s.font_file.to_lowercase().contains("bold"))
                });
                let is_italic = txt.style.to_lowercase().contains("italic") || txt.style.to_lowercase().contains("oblique") || txt.oblique_angle.abs() > 0.05;
                let font_name = self.select_font_name(is_bold, is_italic);

                let decoded = decode_cad_text_escapes(&txt.value);
                let escaped = encode_pdf_string(&decoded);

                let total_rot = txt.rotation + transform.rotation_radians();
                let cos = total_rot.cos();
                let sin = total_rot.sin();

                let wf = if txt.width_factor > 0.01 && (txt.width_factor - 1.0).abs() > 0.01 {
                    txt.width_factor
                } else {
                    1.0
                };

                if (wf - 1.0).abs() > 0.01 {
                    let tz = (wf * 100.0).clamp(10.0, 500.0);
                    stream.push_str(&format!(
                        "BT\n/{} {:.2} Tf\n{:.1} Tz\n{:.4} {:.4} {:.4} {:.4} {:.2} {:.2} Tm\n({}) Tj\n100 Tz\nET\n",
                        font_name, font_size,
                        tz,
                        cos, sin, -sin, cos, px, py,
                        escaped
                    ));
                } else {
                    stream.push_str(&format!(
                        "BT\n/{} {:.2} Tf\n{:.4} {:.4} {:.4} {:.4} {:.2} {:.2} Tm\n({}) Tj\nET\n",
                        font_name, font_size,
                        cos, sin, -sin, cos, px, py,
                        escaped
                    ));
                }
            }
            EntityType::MText(mtext) => {
                Self::ensure_fill(stream, state, r, g, b);
                let (px, py) = transform.apply(
                    mtext.insertion_point.x,
                    mtext.insertion_point.y,
                );
                let base_font_size = (mtext.height * transform.scale()).max(0.05);

                let mtext_val = self.resolve_mtext_value(mtext);
                let mut details = parse_mtext_details(&mtext_val, mtext.rectangle_width, mtext.height);

                let style_is_bold = mtext.style.to_lowercase().contains("bold") || self.doc.text_styles.iter().any(|s| {
                    s.name.eq_ignore_ascii_case(&mtext.style) && (s.name.to_lowercase().contains("bold") || s.font_file.to_lowercase().contains("bold"))
                });
                let style_is_italic = mtext.style.to_lowercase().contains("italic") || mtext.style.to_lowercase().contains("oblique") || self.doc.text_styles.iter().any(|s| {
                    s.name.eq_ignore_ascii_case(&mtext.style) && (s.name.to_lowercase().contains("italic") || s.font_file.to_lowercase().contains("italic"))
                });
                if style_is_bold {
                    for line in &mut details.lines {
                        if !line.is_bold && !mtext_val.contains("|b0") && !mtext_val.contains("|B0") {
                            line.is_bold = true;
                        }
                    }
                }
                if style_is_italic {
                    for line in &mut details.lines {
                        if !line.is_italic && !mtext_val.contains("|i0") && !mtext_val.contains("|I0") {
                            line.is_italic = true;
                        }
                    }
                }

                let total_rot = mtext.rotation + transform.rotation_radians();
                let cos = total_rot.cos();
                let sin = total_rot.sin();

                let attach_val = mtext.attachment_point as u8;
                let is_center_h = attach_val == 2 || attach_val == 5 || attach_val == 8;
                let is_right_h = attach_val == 3 || attach_val == 6 || attach_val == 9;

                let num_lines = details.lines.len().max(1);
                let line_font_sizes: Vec<f64> = details.lines.iter().map(|l| {
                    (base_font_size * l.height_scale).max(0.05)
                }).collect();

                let line_spacings: Vec<f64> = line_font_sizes.iter().map(|&sz| sz * 1.3).collect();
                let cap_height_0 = line_font_sizes.first().copied().unwrap_or(base_font_size) * 0.72;
                let cap_height_last = line_font_sizes.last().copied().unwrap_or(base_font_size) * 0.72;

                let total_block_h = if num_lines > 1 {
                    line_spacings[..num_lines - 1].iter().sum::<f64>() + cap_height_last
                } else {
                    cap_height_0
                };

                let v_offset_line0 = if attach_val >= 4 && attach_val <= 6 {
                    total_block_h / 2.0 - cap_height_0
                } else if attach_val >= 7 {
                    if num_lines > 1 {
                        line_spacings[..num_lines - 1].iter().sum::<f64>()
                    } else {
                        0.0
                    }
                } else {
                    -line_font_sizes.first().copied().unwrap_or(base_font_size)
                };

                let mut cum_y = 0.0;
                for (idx, line_info) in details.lines.iter().enumerate() {
                    let font_size = line_font_sizes[idx];
                    if idx > 0 {
                        cum_y += line_spacings[idx - 1];
                    }
                    if line_info.text.is_empty() {
                        continue;
                    }
                    let font_name = self.select_font_name(line_info.is_bold, line_info.is_italic);

                    let wf = line_info.width_factor;
                    let tf = line_info.tracking_factor;
                    let mut char_space = if (tf - 1.0).abs() > 0.01 {
                        (tf - 1.0) * font_size * 0.55
                    } else {
                        0.0
                    };

                    let num_chars = line_info.text.chars().count();
                    let tracking_total = char_space * (num_chars.saturating_sub(1) as f64);
                    let base_w = estimate_str_width(&line_info.text, font_size) * wf;
                    let mut line_w_pt = base_w + tracking_total;

                    let indent_pt = line_info.indent * transform.scale();
                    let rect_w_pt = (mtext.rectangle_width * transform.scale()).max(0.0);

                    if line_info.alignment == MTextAlignment::Distributed && rect_w_pt > 0.0 && num_chars > 1 {
                        let available_w = (rect_w_pt - indent_pt).max(base_w);
                        if available_w > base_w {
                            char_space = (available_w - base_w) / (num_chars - 1) as f64;
                            line_w_pt = available_w;
                        }
                    }

                    let h_offset = match line_info.alignment {
                        MTextAlignment::Distributed => {
                            if is_center_h {
                                -line_w_pt / 2.0 + indent_pt
                            } else if is_right_h {
                                -line_w_pt + indent_pt
                            } else {
                                indent_pt
                            }
                        }
                        MTextAlignment::Center => {
                            if rect_w_pt > 0.0 {
                                (rect_w_pt - line_w_pt) / 2.0 + indent_pt
                            } else if is_center_h {
                                -line_w_pt / 2.0 + indent_pt
                            } else if is_right_h {
                                -line_w_pt + indent_pt
                            } else {
                                indent_pt
                            }
                        }
                        MTextAlignment::Right => {
                            if rect_w_pt > 0.0 {
                                rect_w_pt - line_w_pt + indent_pt
                            } else {
                                -line_w_pt + indent_pt
                            }
                        }
                        _ => {
                            if is_center_h {
                                -line_w_pt / 2.0 + indent_pt
                            } else if is_right_h {
                                -line_w_pt + indent_pt
                            } else {
                                indent_pt
                            }
                        }
                    };

                    let line_y_local = v_offset_line0 - cum_y;
                    let line_px = px + cos * h_offset - sin * line_y_local;
                    let line_py = py + sin * h_offset + cos * line_y_local;

                    let escaped = encode_pdf_string(&line_info.text);

                    let mut text_ops = String::new();
                    let mut reset_ops = String::new();
                    if char_space.abs() > 0.001 {
                        text_ops.push_str(&format!("{:.2} Tc\n", char_space));
                        reset_ops.push_str("0 Tc\n");
                    }
                    if (wf - 1.0).abs() > 0.01 {
                        let tz = (wf * 100.0).clamp(10.0, 500.0);
                        text_ops.push_str(&format!("{:.1} Tz\n", tz));
                        reset_ops.push_str("100 Tz\n");
                    }

                    stream.push_str(&format!(
                        "BT\n/{} {:.2} Tf\n{}{:.4} {:.4} {:.4} {:.4} {:.2} {:.2} Tm\n({}) Tj\n{}ET\n",
                        font_name, font_size,
                        text_ops,
                        cos, sin, -sin, cos, line_px, line_py,
                        escaped,
                        reset_ops
                    ));
                }
            }
            EntityType::Hatch(hatch) => {
                if hatch.paths.is_empty() {
                    return;
                }
                if hatch.is_solid {
                    Self::ensure_fill(stream, state, r, g, b);
                } else {
                    Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                }

                let mut has_geometry = false;
                for path in &hatch.paths {
                    if path.edges.is_empty() {
                        continue;
                    }
                    has_geometry = true;
                    let mut is_first = true;
                    for edge in &path.edges {
                        match edge {
                            BoundaryEdge::Line(line) => {
                                let (p1x, p1y) = transform.apply(line.start.x, line.start.y);
                                let (p2x, p2y) = transform.apply(line.end.x, line.end.y);
                                if is_first {
                                    stream.push_str(&format!("{:.2} {:.2} m\n", p1x, p1y));
                                    is_first = false;
                                }
                                stream.push_str(&format!("{:.2} {:.2} l\n", p2x, p2y));
                            }
                            BoundaryEdge::CircularArc(arc) => {
                                let (cx, cy) = transform.apply(arc.center.x, arc.center.y);
                                let r_rad = arc.radius * transform.scale();
                                let rot = transform.rotation_radians();
                                let start_rad = arc.start_angle + rot;
                                let end_rad = arc.end_angle + rot;
                                is_first = false;
                                stream.push_str(&generate_arc_path(cx, cy, r_rad, start_rad, end_rad));
                            }
                            BoundaryEdge::EllipticArc(el) => {
                                let (cx, cy) = transform.apply(el.center.x, el.center.y);
                                let maj_x = el.major_axis_endpoint.x * transform.a + el.major_axis_endpoint.y * transform.c;
                                let maj_y = el.major_axis_endpoint.x * transform.b + el.major_axis_endpoint.y * transform.d;
                                stream.push_str(&generate_ellipse_path(
                                    cx,
                                    cy,
                                    maj_x,
                                    maj_y,
                                    el.minor_axis_ratio,
                                    el.start_angle,
                                    el.end_angle,
                                ));
                                is_first = false;
                            }
                            BoundaryEdge::Spline(sp) => {
                                if !sp.control_points.is_empty() {
                                    let first = &sp.control_points[0];
                                    let (p0_x, p0_y) = transform.apply(first.x, first.y);
                                    if is_first {
                                        stream.push_str(&format!("{:.2} {:.2} m\n", p0_x, p0_y));
                                        is_first = false;
                                    }
                                    for pt in &sp.control_points[1..] {
                                        let (px, py) = transform.apply(pt.x, pt.y);
                                        stream.push_str(&format!("{:.2} {:.2} l\n", px, py));
                                    }
                                }
                            }
                            BoundaryEdge::Polyline(poly) => {
                                if !poly.vertices.is_empty() {
                                    let first = &poly.vertices[0];
                                    let (p0_x, p0_y) = transform.apply(first.x, first.y);
                                    if is_first {
                                        stream.push_str(&format!("{:.2} {:.2} m\n", p0_x, p0_y));
                                        is_first = false;
                                    } else {
                                        stream.push_str(&format!("{:.2} {:.2} l\n", p0_x, p0_y));
                                    }
                                    for v in &poly.vertices[1..] {
                                        let (px, py) = transform.apply(v.x, v.y);
                                        stream.push_str(&format!("{:.2} {:.2} l\n", px, py));
                                    }
                                }
                            }
                        }
                    }
                    stream.push_str("h\n");
                }

                if has_geometry {
                    if hatch.is_solid {
                        stream.push_str("f*\n");
                    } else {
                        stream.push_str("S\n");
                    }
                }
            }
            EntityType::Spline(spline) => {
                if spline.control_points.len() >= 2 {
                    Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                    let first = &spline.control_points[0];
                    let (p0x, p0y) = transform.apply(first.x, first.y);
                    stream.push_str(&format!("{:.2} {:.2} m\n", p0x, p0y));

                    for pt in &spline.control_points[1..] {
                        let (px, py) = transform.apply(pt.x, pt.y);
                        stream.push_str(&format!("{:.2} {:.2} l\n", px, py));
                    }
                    if spline.flags.closed {
                        stream.push_str("h ");
                    }
                    stream.push_str("S\n");
                }
            }
            EntityType::Leader(ldr) => {
                if ldr.vertices.len() >= 2 {
                    Self::ensure_stroke(stream, state, r, g, b, line_width_pt);
                    let first = &ldr.vertices[0];
                    let (p0x, p0y) = transform.apply(first.x, first.y);
                    stream.push_str(&format!("{:.2} {:.2} m\n", p0x, p0y));

                    for v in &ldr.vertices[1..] {
                        let (px, py) = transform.apply(v.x, v.y);
                        stream.push_str(&format!("{:.2} {:.2} l\n", px, py));
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
                        self.render_entity(block_ent, &combined, stream, state, visited_blocks, depth + 1);
                    }
                    visited_blocks.remove(block_name);
                }
            }
            EntityType::Dimension(dim) => {
                let block_name = &dim.base().block_name;
                if !block_name.is_empty() && visited_blocks.insert(block_name.clone()) {
                    for block_ent in self.doc.entities_in_block(block_name) {
                        self.render_entity(block_ent, transform, stream, state, visited_blocks, depth + 1);
                    }
                    visited_blocks.remove(block_name);
                }
            }
            _ => {}
        }
    }
}

fn char_to_winansi(ch: char) -> Option<u8> {
    match ch as u32 {
        0x00..=0x7F => Some(ch as u8),
        0x80 | 0x20AC => Some(0x80), // €
        0x201A => Some(0x82),        // ‚
        0x0192 => Some(0x83),        // ƒ
        0x201E => Some(0x84),        // „
        0x2026 => Some(0x85),        // …
        0x2020 => Some(0x86),        // †
        0x2021 => Some(0x87),        // ‡
        0x02C6 => Some(0x88),        // ˆ
        0x2030 => Some(0x89),        // ‰
        0x0160 => Some(0x8A),        // Š
        0x2039 => Some(0x8B),        // ‹
        0x0152 => Some(0x8C),        // Œ
        0x017D => Some(0x8E),        // Ž
        0x2018 => Some(0x91),        // ‘
        0x2019 => Some(0x92),        // ’
        0x201C => Some(0x93),        // “
        0x201D => Some(0x94),        // ”
        0x2022 => Some(0x95),        // •
        0x2013 => Some(0x96),        // –
        0x2014 => Some(0x97),        // —
        0x02DC => Some(0x98),        // ˜
        0x2122 => Some(0x99),        // ™
        0x0161 => Some(0x9A),        // š
        0x203A => Some(0x9B),        // ›
        0x0153 => Some(0x9C),        // œ
        0x017E => Some(0x9E),        // ž
        0x0178 => Some(0x9F),        // Ÿ
        0xA0..=0xFF => Some((ch as u32) as u8),
        _ => None,
    }
}

fn encode_pdf_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for ch in s.chars() {
        match ch {
            '(' => out.push_str(r"\("),
            ')' => out.push_str(r"\)"),
            '\\' => out.push_str(r"\\"),
            '\r' => out.push_str(r"\r"),
            '\n' => out.push_str(r"\n"),
            '\t' => out.push_str(r"\t"),
            ' '..='~' => out.push(ch),
            _ => {
                if ch == '\u{2028}' || ch == '\u{2029}' || ch == '\u{feff}' || ch == '\u{200b}' {
                    // Zero-width or unicode line breaks - do not emit '?'
                    continue;
                }
                if let Some(byte) = char_to_winansi(ch) {
                    out.push_str(&format!("\\{:03o}", byte));
                } else if ch.is_whitespace() || ch.is_control() {
                    out.push(' ');
                } else {
                    out.push('?');
                }
            }
        }
    }
    out
}
