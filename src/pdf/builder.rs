use crate::cad::bounds::{
    calculate_block_bounds, calculate_document_bounds, entity_intersects_bbox,
    is_non_model_space_entity, BoundingBox, Transform2D,
};
use crate::cad::color::{aci_to_rgb, parse_hex_color, resolve_color, rgb_to_pdf};
use crate::cad::linetype::LineDashPattern;
use crate::cad::mtext::{
    decode_cad_text_escapes, estimate_str_width, estimate_str_width_category,
    parse_mtext_details_with_shx, FontCategory, MTextAlignment,
};
use crate::error::{ConversionError, ConversionResult};
use crate::options::{ColorMode, ConversionMode, ConversionOptions, Orientation, PageSize};
use crate::pdf::geometry::{
    generate_arc_path_with_mode, generate_bulge_segment, generate_circle_path,
    generate_ellipse_path, generate_ellipse_path_with_mode, sample_bspline, Point2D,
};

use acadrust::document::CadDocument;
use acadrust::entities::{BoundaryEdge, EntityType};
use acadrust::entities::ole_presentation::OlePresentation;
use acadrust::objects::ObjectType;
use acadrust::types::color::Color;
use acadrust::types::Matrix3;
use acadrust::{Handle, Vector3};

use hipdf::fonts::{Font, FontManager, StandardFont};
use hipdf::lopdf::{Dictionary, Document, Object, Stream};
use hipdf::ocg::OCGManager;

use std::collections::{HashMap, HashSet};

#[derive(Default, Clone, PartialEq)]
pub struct GraphicsState {
    pub stroke_rgb: Option<(f32, f32, f32)>,
    pub fill_rgb: Option<(f32, f32, f32)>,
    pub line_width: Option<f64>,
    pub dash_pattern: Option<Vec<f32>>,
}

pub struct CadToPdfBuilder<'a> {
    doc: &'a CadDocument,
    options: ConversionOptions,
    bbox: BoundingBox,
    pdf_doc: Document,
    ocg_manager: OCGManager,
    layer_ocg_map: HashMap<String, String>,
    font_manager: FontManager,
    embedded_fonts: HashMap<&'static str, ((u32, u16), String)>,
    layer_colors: HashMap<String, Color>,
    layer_linetypes: HashMap<String, String>,
    default_draw_rgb: (u8, u8, u8),
    block_bounds_cache: HashMap<String, BoundingBox>,
    active_frozen_layers: Option<HashSet<Handle>>,
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
        let mut layer_linetypes = HashMap::new();
        for layer in doc.layers.iter() {
            let key = layer.name.to_ascii_lowercase();
            layer_colors.insert(key.clone(), layer.color.clone());
            layer_linetypes.insert(key, layer.line_type.clone());
        }

        let mut pdf_doc = Document::with_version("1.7");
        let font_manager = FontManager::new();
        let embedded_fonts = HashMap::new();

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
                layer_ocg_map.insert(layer.name.to_ascii_lowercase(), tag);
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
            embedded_fonts,
            layer_colors,
            layer_linetypes,
            default_draw_rgb,
            block_bounds_cache: HashMap::new(),
            active_frozen_layers: None,
        })
    }

    pub fn resolve_font_category(&self, style_name: &str) -> FontCategory {
        let style_opt = self.doc.text_styles.iter().find(|s| s.name.eq_ignore_ascii_case(style_name));
        let font_file = style_opt.map(|s| s.font_file.to_lowercase()).unwrap_or_default();
        let ttf = style_opt.map(|s| s.true_type_font.to_lowercase()).unwrap_or_default();
        let name_lower = style_name.to_lowercase();

        if ttf.contains("times") || ttf.contains("century") || ttf.contains("georgia") || ttf.contains("garamond")
            || font_file.contains("times")
            || (font_file.contains("roman") && !font_file.contains("romans") && !font_file.contains("romanc"))
            || name_lower.contains("times")
        {
            return FontCategory::Serif;
        }

        if ttf.contains("arial") || ttf.contains("helvetica") || ttf.contains("calibri") || ttf.contains("tahoma")
            || ttf.contains("verdana") || ttf.contains("sans")
            || font_file.contains("arial") || font_file.contains("helvetica") || font_file.contains("calibri")
            || name_lower.contains("arial") || name_lower.contains("helvetica")
        {
            return FontCategory::SansSerif;
        }

        if ttf.contains("mono") || ttf.contains("courier") || ttf.contains("consolas") || ttf.contains("source code")
            || font_file.contains("mono") || font_file.contains("courier") || font_file.contains("consolas")
            || font_file.starts_with("txt") || font_file == "txt.shx" || font_file == "txt"
            || name_lower.contains("mono") || name_lower.contains("courier") || name_lower.contains("consolas")
            || (name_lower == "standard" && font_file.is_empty() && ttf.is_empty())
            || (style_opt.is_none() && (name_lower.is_empty() || name_lower == "standard"))
        {
            return FontCategory::Monospace;
        }

        // Standard AutoCAD fonts (txt.shx, simplex.shx, romans.shx, etc.) and TrueType fonts
        // default to proportional SansSerif (Helvetica in PDF)
        FontCategory::SansSerif
    }

    fn standard_font(category: FontCategory, is_bold: bool, is_italic: bool) -> StandardFont {
        match category {
            FontCategory::SansSerif => match (is_bold, is_italic) {
                (true, true) => StandardFont::HelveticaBoldOblique,
                (true, false) => StandardFont::HelveticaBold,
                (false, true) => StandardFont::HelveticaOblique,
                (false, false) => StandardFont::Helvetica,
            },
            FontCategory::Monospace => match (is_bold, is_italic) {
                (true, true) => StandardFont::CourierBoldOblique,
                (true, false) => StandardFont::CourierBold,
                (false, true) => StandardFont::CourierOblique,
                (false, false) => StandardFont::Courier,
            },
            FontCategory::Serif => match (is_bold, is_italic) {
                (true, true) => StandardFont::TimesBoldItalic,
                (true, false) => StandardFont::TimesBold,
                (false, true) => StandardFont::TimesItalic,
                (false, false) => StandardFont::TimesRoman,
            },
        }
    }

    fn ensure_font(&mut self, sf: StandardFont) -> String {
        let key = match sf {
            StandardFont::Helvetica => "Helvetica",
            StandardFont::HelveticaBold => "Helvetica-Bold",
            StandardFont::HelveticaOblique => "Helvetica-Oblique",
            StandardFont::HelveticaBoldOblique => "Helvetica-BoldOblique",
            StandardFont::Courier => "Courier",
            StandardFont::CourierBold => "Courier-Bold",
            StandardFont::CourierOblique => "Courier-Oblique",
            StandardFont::CourierBoldOblique => "Courier-BoldOblique",
            StandardFont::TimesRoman => "Times-Roman",
            StandardFont::TimesBold => "Times-Bold",
            StandardFont::TimesItalic => "Times-Italic",
            StandardFont::TimesBoldItalic => "Times-BoldItalic",
            _ => "Helvetica",
        };
        if let Some((_, tag)) = self.embedded_fonts.get(key) {
            return tag.clone();
        }
        let (id, tag) = self.font_manager
            .embed_font(&mut self.pdf_doc, Font::standard(sf))
            .expect("embed standard font");
        self.embedded_fonts.insert(key, (id, tag.clone()));
        tag
    }

    fn select_font_name(&mut self, category: FontCategory, is_bold: bool, is_italic: bool) -> String {
        let sf = Self::standard_font(category, is_bold, is_italic);
        self.ensure_font(sf)
    }

    fn add_all_font_resources(&mut self, resources: &mut Dictionary) {
        for (id, tag) in self.embedded_fonts.values() {
            self.font_manager.add_to_resources(resources, *id, tag);
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
                        if !full_text.is_empty() && (mtext.value.is_empty() || full_text.len() > mtext.value.len()) {
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
                // Conversely, if drawing units are small (inches, meters, or small mechanical parts where max_dim < 595.28 pt),
                // scale page up proportionally so maximum dimension is at least 595.28 pt (ISO A4) to prevent postage-stamp sizing.
                let max_dim = w.max(h);
                if max_dim > 3370.39 {
                    let shrink = 3370.39 / max_dim;
                    w *= shrink;
                    h *= shrink;
                } else if max_dim < 595.28 && max_dim > 0.0 {
                    let expand = 595.28 / max_dim;
                    w *= expand;
                    h *= expand;
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

        // 3. By standard AutoCAD paper space naming convention, without stealing another layout's block
        let mut candidate_names = Vec::new();
        if layout.tab_order <= 1 {
            candidate_names.push("*Paper_Space".to_string());
            candidate_names.push("*Paper_Space0".to_string());
        } else {
            candidate_names.push(format!("*Paper_Space{}", layout.tab_order - 1));
            candidate_names.push(format!("*Paper_Space{}", layout.tab_order));
        }

        for name in &candidate_names {
            if let Some(b) = self.doc.block_records.iter().find(|b| b.name.eq_ignore_ascii_case(name)) {
                let owned_by_other_layout = (b.layout.value() != 0 && b.layout != layout.handle)
                    || self.doc.objects.values().any(|obj| {
                        matches!(
                            obj,
                            ObjectType::Layout(other)
                                if other.handle != layout.handle
                                    && other.block_record.value() != 0
                                    && other.block_record == b.handle
                        )
                    });
                if !owned_by_other_layout {
                    return Some(b.name.clone());
                }
            }
        }

        None
    }

    /// Retrieves entities in a block sorted by SortEntitiesTable draw order (or database order).
    fn get_sorted_entities_in_block(&self, block_name: &str) -> Vec<&'a EntityType> {
        let block_rec = self.doc.block_records.get(block_name);
        let sort_table = block_rec.and_then(|br| {
            self.doc.objects.values().find_map(|obj| {
                if let ObjectType::SortEntitiesTable(ref st) = obj {
                    if st.block_owner_handle == br.handle {
                        return Some(st);
                    }
                }
                None
            })
        }).or_else(|| {
            if block_name.eq_ignore_ascii_case("*Model_Space") {
                self.doc.objects.values().find_map(|obj| {
                    if let ObjectType::SortEntitiesTable(ref st) = obj {
                        Some(st)
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        });

        let is_model = block_name.eq_ignore_ascii_case("*Model_Space");
        let mut entities: Vec<&'a EntityType> = if let Some(br) = block_rec {
            br.entity_handles
                .iter()
                .filter_map(|h| self.doc.get_entity(*h))
                .filter(|e| !(is_model && is_non_model_space_entity(e)))
                .collect()
        } else {
            self.doc
                .entities_in_block(block_name)
                .filter(|e| !(is_model && is_non_model_space_entity(e)))
                .collect()
        };

        if let Some(st) = sort_table {
            entities.sort_by_key(|e| {
                let h = e.common().handle;
                st.get_sort_handle(h).unwrap_or(h).value()
            });
        }

        entities
    }

    /// Returns true if an entity is visible and plottable on its layer (excluding empty/invisible entities).
    fn is_entity_renderable(&self, entity: &EntityType, frozen_handles: Option<&HashSet<Handle>>) -> bool {
        let dyn_ent = entity.as_entity();
        if dyn_ent.is_invisible() {
            return false;
        }
        let layer_name = dyn_ent.layer();
        if layer_name.eq_ignore_ascii_case("defpoints") {
            return false;
        }
        if let Some(layer) = self.doc.layers.iter().find(|l| l.name.eq_ignore_ascii_case(layer_name)) {
            if !layer.is_plottable
                || layer.is_frozen()
                || layer.is_off()
                || frozen_handles.map_or(false, |fh| fh.contains(&layer.handle))
            {
                return false;
            }
        }
        match entity {
            EntityType::Text(t) => !t.value.trim().is_empty(),
            EntityType::MText(m) => !self.resolve_mtext_value(m).trim().is_empty(),
            EntityType::Insert(ins) => {
                !ins.attributes.is_empty() || self.doc.entities_in_block(&ins.block_name).next().is_some()
            }
            _ => true,
        }
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
        let mut active_vp_boxes: Vec<(BoundingBox, HashSet<Handle>)> = Vec::new();

        for entity in self.doc.entities_in_block(&block_name) {
            match entity {
                EntityType::Viewport(vp) => {
                    let is_overall = vp.id == 1 || (
                        (vp.center.x - vp.view_center.x).abs() < 1e-3
                            && (vp.center.y - vp.view_center.y).abs() < 1e-3
                            && (vp.height - vp.view_height).abs() < 1e-3
                    );
                    if !is_overall && vp.status.is_on && vp.width > 0.0 && vp.height > 0.0 && vp.view_height > 0.0 {
                        let aspect = vp.width / vp.height;
                        let view_w = vp.view_height * aspect;
                        let twist_rad = -vp.twist_angle;
                        let cos_tw = twist_rad.cos();
                        let sin_tw = twist_rad.sin();
                        let model_cx = vp.view_target.x + vp.view_center.x * cos_tw - vp.view_center.y * sin_tw;
                        let model_cy = vp.view_target.y + vp.view_center.x * sin_tw + vp.view_center.y * cos_tw;
                        let extent_x = (view_w * cos_tw.abs() + vp.view_height * sin_tw.abs()) / 2.0;
                        let extent_y = (view_w * sin_tw.abs() + vp.view_height * cos_tw.abs()) / 2.0;
                        let vp_box = BoundingBox {
                            min_x: model_cx - extent_x,
                            max_x: model_cx + extent_x,
                            min_y: model_cy - extent_y,
                            max_y: model_cy + extent_y,
                        };
                        let frozen: HashSet<Handle> = vp.frozen_layers.iter().copied().collect();
                        active_vp_boxes.push((vp_box, frozen));
                    }
                }
                _ => {
                    if self.is_entity_renderable(entity, None) {
                        has_sheet_entities = true;
                    }
                }
            }
        }

        // A layout is enabled and visible if it contains visible sheet entities (title block, annotations, geometry)
        // or active viewports looking into visible model space geometry.
        if !has_sheet_entities {
            if active_vp_boxes.is_empty() {
                return false;
            }

            let mut cache = HashMap::new();
            let mut model_hits = 0;
            for entity in self.doc.entities_in_block("*Model_Space") {
                if is_non_model_space_entity(entity) {
                    continue;
                }
                for (vp_box, frozen) in &active_vp_boxes {
                    if !self.is_entity_renderable(entity, Some(frozen)) {
                        continue;
                    }
                    if crate::cad::bounds::entity_intersects_bbox(self.doc, entity, vp_box, &mut cache) {
                        model_hits += 1;
                        if model_hits >= 20 {
                            return true;
                        }
                    }
                }
            }

            return false;
        }

        true
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
                    if let Some(page_id) = self.build_paper_space_page(&l)? {
                        page_ids.push(page_id);
                    }
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
                if let Some(page_id) = self.build_paper_space_page(l)? {
                    page_ids.push(page_id);
                }
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

    /// Builds a single paper space layout page. Returns `None` if no visible content is rendered.
    fn build_paper_space_page(
        &mut self,
        layout: &acadrust::objects::Layout,
    ) -> ConversionResult<Option<(u32, u16)>> {
        // Find block name associated with this layout
        let block_name = match self.find_layout_block_name(layout) {
            Some(name) => name,
            None => return Ok(None),
        };

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
        let block_entities = self.get_sorted_entities_in_block(&block_name);
        let mut viewports = Vec::new();
        let mut sheet_entities: Vec<&EntityType> = Vec::new();

        for entity in block_entities {
            if let EntityType::Viewport(vp) = entity {
                viewports.push(vp);
            } else {
                sheet_entities.push(entity);
            }
        }

        let model_entities = self.get_sorted_entities_in_block("*Model_Space");

        let mut has_rendered_content = false;

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
            let vp_stream = format!(
                "q\n{:.2} {:.2} {:.2} {:.2} re W n\n",
                clip_x, clip_y, vp_w, vp_h
            );

            let mut current_layer_tag: Option<String> = None;
            let mut vp_content = String::new();
            let mut gstate = GraphicsState::default();
            let mut visited_blocks = HashSet::new();

            self.active_frozen_layers = Some(frozen_handles.clone());

            for entity in &model_entities {
                let layer_name = entity.as_entity().layer();
                if layer_name.eq_ignore_ascii_case("defpoints") {
                    continue;
                }
                if let Some(layer) = self.doc.layers.iter().find(|l| l.name.eq_ignore_ascii_case(layer_name)) {
                    if !layer.is_plottable || frozen_handles.contains(&layer.handle) || layer.is_frozen() || layer.is_off() {
                        continue;
                    }
                }

                // Spatial culling: skip entities outside the viewport model box
                if !entity_intersects_bbox(self.doc, entity, &vp_model_box, &mut self.block_bounds_cache) {
                    continue;
                }

                let mut ent_stream = String::new();
                self.render_entity(
                    entity,
                    &vp_transform,
                    &mut ent_stream,
                    &mut gstate,
                    &mut visited_blocks,
                    0,
                    None,
                    None,
                );

                if ent_stream.is_empty() {
                    continue;
                }

                if self.options.export_layers {
                    let tag = self.layer_ocg_map.get(&layer_name.to_ascii_lowercase()).cloned();
                    if tag != current_layer_tag {
                        if current_layer_tag.is_some() {
                            vp_content.push_str("EMC\n");
                        }
                        if let Some(ref t) = tag {
                            vp_content.push_str(&format!("/OC /{} BDC\n", t));
                        }
                        current_layer_tag = tag;
                    }
                }
                vp_content.push_str(&ent_stream);
            }

            self.active_frozen_layers = None;

            if current_layer_tag.is_some() {
                vp_content.push_str("EMC\n");
            }

            if !vp_content.is_empty() {
                has_rendered_content = true;
                page_stream.push_str(&vp_stream);
                page_stream.push_str(&vp_content);
                page_stream.push_str("Q\n");
            }
        }

        // 3. Render sheet entities on top of viewports (title blocks, borders, text)
        let mut current_layer_tag: Option<String> = None;
        let mut sheet_content = String::new();
        let mut gstate = GraphicsState::default();
        let mut visited_blocks = HashSet::new();

        for entity in sheet_entities {
            let layer_name = entity.as_entity().layer();
            if layer_name.eq_ignore_ascii_case("defpoints") {
                continue;
            }
            if let Some(layer) = self.doc.layers.iter().find(|l| l.name.eq_ignore_ascii_case(layer_name)) {
                if !layer.is_plottable || layer.is_frozen() || layer.is_off() {
                    continue;
                }
            }

            let mut ent_stream = String::new();
            self.render_entity(
                entity,
                &page_transform,
                &mut ent_stream,
                &mut gstate,
                &mut visited_blocks,
                0,
                None,
                None,
            );

            if ent_stream.is_empty() {
                continue;
            }

            if self.options.export_layers {
                let tag = self.layer_ocg_map.get(&layer_name.to_ascii_lowercase()).cloned();
                if tag != current_layer_tag {
                    if current_layer_tag.is_some() {
                        sheet_content.push_str("EMC\n");
                    }
                    if let Some(ref t) = tag {
                        sheet_content.push_str(&format!("/OC /{} BDC\n", t));
                    }
                    current_layer_tag = tag;
                }
            }
            sheet_content.push_str(&ent_stream);
        }

        if current_layer_tag.is_some() {
            sheet_content.push_str("EMC\n");
        }

        if !sheet_content.is_empty() {
            has_rendered_content = true;
        }
        if !has_rendered_content {
            return Ok(None);
        }

        page_stream.push_str(&sheet_content);
        page_stream.push_str("Q\n");

        let stream = Stream::new(Dictionary::new(), page_stream.into_bytes());
        let content_id = self.pdf_doc.add_object(Object::Stream(stream));

        let mut resources = Dictionary::new();
        self.add_all_font_resources(&mut resources);

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
        Ok(Some(page_id))
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

        let model_entities = self.get_sorted_entities_in_block("*Model_Space");
        let mut current_layer_tag: Option<String> = None;
        let mut gstate = GraphicsState::default();
        let mut visited_blocks = HashSet::new();

        for entity in model_entities {
            let layer_name = entity.as_entity().layer();
            if layer_name.eq_ignore_ascii_case("defpoints") {
                continue;
            }
            if let Some(layer) = self.doc.layers.iter().find(|l| l.name.eq_ignore_ascii_case(layer_name)) {
                if !layer.is_plottable || layer.is_frozen() || layer.is_off() {
                    continue;
                }
            }

            let mut ent_stream = String::new();
            self.render_entity(
                entity,
                &page_transform,
                &mut ent_stream,
                &mut gstate,
                &mut visited_blocks,
                0,
                None,
                None,
            );

            if ent_stream.is_empty() {
                continue;
            }

            if self.options.export_layers {
                let tag = self.layer_ocg_map.get(&layer_name.to_ascii_lowercase()).cloned();
                if tag != current_layer_tag {
                    if current_layer_tag.is_some() {
                        page_stream.push_str("EMC\n");
                    }
                    if let Some(ref t) = tag {
                        page_stream.push_str(&format!("/OC /{} BDC\n", t));
                    }
                    current_layer_tag = tag;
                }
            }
            page_stream.push_str(&ent_stream);
        }

        if current_layer_tag.is_some() {
            page_stream.push_str("EMC\n");
        }

        page_stream.push_str("Q\n");

        let stream = Stream::new(Dictionary::new(), page_stream.into_bytes());
        let content_id = self.pdf_doc.add_object(Object::Stream(stream));

        let mut resources = Dictionary::new();
        self.add_all_font_resources(&mut resources);

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
    fn ensure_stroke(
        stream: &mut String,
        state: &mut GraphicsState,
        r: f32,
        g: f32,
        b: f32,
        line_width_pt: f64,
        dash_pattern: Option<&[f32]>,
    ) {
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
        let current_dash = state.dash_pattern.as_deref();
        let desired_dash = dash_pattern.unwrap_or(&[]);
        if current_dash != Some(desired_dash) {
            if desired_dash.is_empty() {
                stream.push_str("[] 0 d\n");
                state.dash_pattern = Some(Vec::new());
            } else {
                stream.push_str("[");
                for (i, d) in desired_dash.iter().enumerate() {
                    if i > 0 {
                        stream.push(' ');
                    }
                    stream.push_str(&format!("{:.2}", d));
                }
                stream.push_str("] 0 d\n");
                state.dash_pattern = Some(desired_dash.to_vec());
            }
        }
    }

    #[inline]
    fn ensure_fill(stream: &mut String, state: &mut GraphicsState, r: f32, g: f32, b: f32) {
        if state.fill_rgb != Some((r, g, b)) {
            stream.push_str(&format!("{:.3} {:.3} {:.3} rg\n", r, g, b));
            state.fill_rgb = Some((r, g, b));
        }
    }

fn map_attrib_h_align(h: acadrust::entities::attribute_definition::HorizontalAlignment) -> acadrust::entities::text::TextHorizontalAlignment {
    match h {
        acadrust::entities::attribute_definition::HorizontalAlignment::Left => acadrust::entities::text::TextHorizontalAlignment::Left,
        acadrust::entities::attribute_definition::HorizontalAlignment::Center => acadrust::entities::text::TextHorizontalAlignment::Center,
        acadrust::entities::attribute_definition::HorizontalAlignment::Right => acadrust::entities::text::TextHorizontalAlignment::Right,
        acadrust::entities::attribute_definition::HorizontalAlignment::Aligned => acadrust::entities::text::TextHorizontalAlignment::Aligned,
        acadrust::entities::attribute_definition::HorizontalAlignment::Middle => acadrust::entities::text::TextHorizontalAlignment::Middle,
        acadrust::entities::attribute_definition::HorizontalAlignment::Fit => acadrust::entities::text::TextHorizontalAlignment::Fit,
    }
}

fn map_attrib_v_align(v: acadrust::entities::attribute_definition::VerticalAlignment) -> acadrust::entities::text::TextVerticalAlignment {
    match v {
        acadrust::entities::attribute_definition::VerticalAlignment::Baseline => acadrust::entities::text::TextVerticalAlignment::Baseline,
        acadrust::entities::attribute_definition::VerticalAlignment::Bottom => acadrust::entities::text::TextVerticalAlignment::Bottom,
        acadrust::entities::attribute_definition::VerticalAlignment::Middle => acadrust::entities::text::TextVerticalAlignment::Middle,
        acadrust::entities::attribute_definition::VerticalAlignment::Top => acadrust::entities::text::TextVerticalAlignment::Top,
    }
}

    fn render_hatch_pattern_lines(
        hatch: &acadrust::entities::Hatch,
        transform: &Transform2D,
        stream: &mut String,
        state: &mut GraphicsState,
        r: f32,
        g: f32,
        b: f32,
        line_width_pt: f64,
    ) {
        if hatch.pattern.lines.is_empty() {
            return;
        }

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for path in &hatch.paths {
            for edge in &path.edges {
                match edge {
                    BoundaryEdge::Line(l) => {
                        min_x = min_x.min(l.start.x).min(l.end.x);
                        min_y = min_y.min(l.start.y).min(l.end.y);
                        max_x = max_x.max(l.start.x).max(l.end.x);
                        max_y = max_y.max(l.start.y).max(l.end.y);
                    }
                    BoundaryEdge::CircularArc(a) => {
                        min_x = min_x.min(a.center.x - a.radius);
                        min_y = min_y.min(a.center.y - a.radius);
                        max_x = max_x.max(a.center.x + a.radius);
                        max_y = max_y.max(a.center.y + a.radius);
                    }
                    BoundaryEdge::EllipticArc(e) => {
                        let rad = e.major_axis_endpoint.x.hypot(e.major_axis_endpoint.y);
                        min_x = min_x.min(e.center.x - rad);
                        min_y = min_y.min(e.center.y - rad);
                        max_x = max_x.max(e.center.x + rad);
                        max_y = max_y.max(e.center.y + rad);
                    }
                    BoundaryEdge::Spline(sp) => {
                        for cp in &sp.control_points {
                            min_x = min_x.min(cp.x);
                            min_y = min_y.min(cp.y);
                            max_x = max_x.max(cp.x);
                            max_y = max_y.max(cp.y);
                        }
                    }
                    BoundaryEdge::Polyline(poly) => {
                        for v in &poly.vertices {
                            min_x = min_x.min(v.x);
                            min_y = min_y.min(v.y);
                            max_x = max_x.max(v.x);
                            max_y = max_y.max(v.y);
                        }
                    }
                }
            }
        }

        if min_x > max_x || min_y > max_y {
            return;
        }

        let diag = (max_x - min_x).hypot(max_y - min_y);
        if diag < 1e-4 {
            return;
        }

        let corners = [
            (min_x, min_y),
            (max_x, min_y),
            (max_x, max_y),
            (min_x, max_y),
        ];

        let mut families = Vec::new();
        for line in &hatch.pattern.lines {
            let ang = line.angle;
            let base_pt = line.base_point;
            let off_x = line.offset.x;
            let off_y = line.offset.y;
            let dashes: Vec<f64> = line.dash_lengths.clone();
            families.push((ang, base_pt.x, base_pt.y, off_x, off_y, dashes.clone()));
            if hatch.is_double {
                let perp_ang = ang + std::f64::consts::FRAC_PI_2;
                families.push((perp_ang, base_pt.x, base_pt.y, -off_y, off_x, dashes));
            }
        }

        let pw = if line_width_pt > 0.0 { line_width_pt.clamp(0.1, 0.75) } else { 0.0 };

        for (ang, bpx, bpy, off_x, off_y, dashes) in families {
            let u = (ang.cos(), ang.sin());
            let v = (-ang.sin(), ang.cos());

            // In AutoCAD DXF/DWG, off_x and off_y are in OCS (the hatch coordinate system).
            // The step perpendicular to the lines (along v) is v . offset:
            let step_v = -off_x * u.1 + off_y * u.0;

            let mut v_min = f64::INFINITY;
            let mut v_max = f64::NEG_INFINITY;
            let mut u_min = f64::INFINITY;
            let mut u_max = f64::NEG_INFINITY;

            for &(cx, cy) in &corners {
                let proj_v = cx * v.0 + cy * v.1;
                let proj_u = cx * u.0 + cy * u.1;
                v_min = v_min.min(proj_v);
                v_max = v_max.max(proj_v);
                u_min = u_min.min(proj_u);
                u_max = u_max.max(proj_u);
            }

            let v_base = bpx * v.0 + bpy * v.1;

            let (k_start, k_end) = if step_v.abs() >= 1e-5 {
                let k1 = (v_min - v_base) / step_v;
                let k2 = (v_max - v_base) / step_v;
                let ks = k1.min(k2).floor() as i64 - 1;
                let ke = k1.max(k2).ceil() as i64 + 1;
                (ks, ke)
            } else if v_base >= v_min && v_base <= v_max {
                (0, 0)
            } else {
                continue;
            };

            let num_lines = (k_end.saturating_sub(k_start)).saturating_add(1).max(0);
            if num_lines > 20000 {
                continue;
            }

            let dash_pattern = if !dashes.is_empty() {
                let arr: Vec<f32> = dashes
                    .iter()
                    .map(|&d| {
                        if d.abs() < 1e-6 {
                            0.1
                        } else {
                            (d.abs() * transform.scale()).max(0.05) as f32
                        }
                    })
                    .collect();
                Some(arr)
            } else {
                None
            };

            Self::ensure_stroke(stream, state, r, g, b, pw, dash_pattern.as_deref());

            let dash_period: f64 = dashes.iter().map(|d| d.abs()).sum();
            let margin = diag * 0.05;

            let mut batch_count = 0;
            for k in k_start..=k_end {
                let k_f = k as f64;
                let origin_x = bpx + k_f * off_x;
                let origin_y = bpy + k_f * off_y;
                let origin_u = origin_x * u.0 + origin_y * u.1;

                let (start_u, end_u) = if dash_period > 1e-4 {
                    let s = ((u_min - origin_u - margin) / dash_period).floor() * dash_period;
                    let e = ((u_max - origin_u + margin) / dash_period).ceil() * dash_period;
                    (s, e)
                } else {
                    (u_min - origin_u - margin, u_max - origin_u + margin)
                };

                let p1_x = origin_x + start_u * u.0;
                let p1_y = origin_y + start_u * u.1;
                let p2_x = origin_x + end_u * u.0;
                let p2_y = origin_y + end_u * u.1;

                let (tx1, ty1) = transform.apply(p1_x, p1_y);
                let (tx2, ty2) = transform.apply(p2_x, p2_y);

                stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l\n", tx1, ty1, tx2, ty2));
                batch_count += 1;
                if batch_count >= 500 {
                    stream.push_str("S\n");
                    batch_count = 0;
                }
            }
            if batch_count > 0 {
                stream.push_str("S\n");
            }
        }
    }

    fn render_hatch_gradient(
        &self,
        hatch: &acadrust::entities::Hatch,
        transform: &Transform2D,
        stream: &mut String,
        state: &mut GraphicsState,
        default_rgb: (f32, f32, f32),
    ) {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for path in &hatch.paths {
            for edge in &path.edges {
                match edge {
                    BoundaryEdge::Line(l) => {
                        let (x1, y1) = transform.apply(l.start.x, l.start.y);
                        let (x2, y2) = transform.apply(l.end.x, l.end.y);
                        min_x = min_x.min(x1).min(x2);
                        max_x = max_x.max(x1).max(x2);
                        min_y = min_y.min(y1).min(y2);
                        max_y = max_y.max(y1).max(y2);
                    }
                    BoundaryEdge::CircularArc(a) => {
                        let (cx, cy) = transform.apply(a.center.x, a.center.y);
                        let r = a.radius.abs() * transform.scale();
                        min_x = min_x.min(cx - r);
                        max_x = max_x.max(cx + r);
                        min_y = min_y.min(cy - r);
                        max_y = max_y.max(cy + r);
                    }
                    BoundaryEdge::EllipticArc(e) => {
                        let (cx, cy) = transform.apply(e.center.x, e.center.y);
                        let r = e.major_axis_endpoint.x.hypot(e.major_axis_endpoint.y) * transform.scale();
                        min_x = min_x.min(cx - r);
                        max_x = max_x.max(cx + r);
                        min_y = min_y.min(cy - r);
                        max_y = max_y.max(cy + r);
                    }
                    BoundaryEdge::Spline(sp) => {
                        for cp in &sp.control_points {
                            let (x, y) = transform.apply(cp.x, cp.y);
                            min_x = min_x.min(x);
                            max_x = max_x.max(x);
                            min_y = min_y.min(y);
                            max_y = max_y.max(y);
                        }
                    }
                    BoundaryEdge::Polyline(poly) => {
                        for v in &poly.vertices {
                            let (x, y) = transform.apply(v.x, v.y);
                            min_x = min_x.min(x);
                            max_x = max_x.max(x);
                            min_y = min_y.min(y);
                            max_y = max_y.max(y);
                        }
                    }
                }
            }
        }

        if min_x > max_x || min_y > max_y {
            return;
        }

        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        let r_diag = (max_x - min_x).hypot(max_y - min_y) * 0.5 * 1.05;
        if r_diag < 1e-4 {
            return;
        }

        let colors: Vec<(f32, f32, f32)> = hatch.gradient_color.colors.iter().map(|entry| {
            let (r8, g8, b8) = resolve_color(&entry.color, None, None, (128, 128, 128));
            let final_rgb = match self.options.color_mode {
                ColorMode::Normal => (r8, g8, b8),
                ColorMode::Monochrome => (0, 0, 0),
                ColorMode::Grayscale => {
                    let gray = (0.299 * r8 as f64 + 0.587 * g8 as f64 + 0.114 * b8 as f64) as u8;
                    (gray, gray, gray)
                }
                ColorMode::Invert => (255 - r8, 255 - g8, 255 - b8),
            };
            rgb_to_pdf(final_rgb)
        }).collect();

        let (c0, c1) = if colors.len() >= 2 {
            (colors[0], colors[1])
        } else if colors.len() == 1 {
            let c = colors[0];
            let tint = (hatch.gradient_color.color_tint as f32).clamp(0.0, 1.0);
            let c_tint = (c.0 * (1.0 - tint) + tint, c.1 * (1.0 - tint) + tint, c.2 * (1.0 - tint) + tint);
            (c, c_tint)
        } else {
            (default_rgb, (1.0, 1.0, 1.0))
        };

        let name = hatch.gradient_color.name.to_ascii_uppercase();

        if name.contains("SPHER") {
            let num_steps = 32;
            for i in (0..=num_steps).rev() {
                let f = i as f64 / num_steps as f64;
                let rad = f * r_diag;
                let t = if name.contains("INV") { f as f32 } else { 1.0 - f as f32 };
                let cr = c0.0 * (1.0 - t) + c1.0 * t;
                let cg = c0.1 * (1.0 - t) + c1.1 * t;
                let cb = c0.2 * (1.0 - t) + c1.2 * t;
                Self::ensure_fill(stream, state, cr, cg, cb);
                stream.push_str(&generate_circle_path(cx, cy, rad));
                stream.push_str("f*\n");
            }
        } else {
            let eff_angle = hatch.gradient_color.angle + transform.rotation_radians();
            let ux = eff_angle.cos();
            let uy = eff_angle.sin();
            let vx = -uy;
            let vy = ux;

            let num_strips = 64;
            let step = (2.0 * r_diag) / num_strips as f64;

            for i in 0..num_strips {
                let u0 = -r_diag + i as f64 * step;
                let u1 = -r_diag + (i + 1) as f64 * step;
                let s = (i as f32 + 0.5) / num_strips as f32;

                let t = if name.contains("INVCYL") {
                    2.0 * (s - 0.5).abs()
                } else if name.contains("CYL") {
                    1.0 - 2.0 * (s - 0.5).abs()
                } else {
                    s
                };

                let cr = c0.0 * (1.0 - t) + c1.0 * t;
                let cg = c0.1 * (1.0 - t) + c1.1 * t;
                let cb = c0.2 * (1.0 - t) + c1.2 * t;
                Self::ensure_fill(stream, state, cr, cg, cb);

                let p1x = cx + u0 * ux - r_diag * vx;
                let p1y = cy + u0 * uy - r_diag * vy;
                let p2x = cx + u1 * ux - r_diag * vx;
                let p2y = cy + u1 * uy - r_diag * vy;
                let p3x = cx + u1 * ux + r_diag * vx;
                let p3y = cy + u1 * uy + r_diag * vy;
                let p4x = cx + u0 * ux + r_diag * vx;
                let p4y = cy + u0 * uy + r_diag * vy;

                stream.push_str(&format!(
                    "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h f*\n",
                    p1x, p1y, p2x, p2y, p3x, p3y, p4x, p4y
                ));
            }
        }
        state.fill_rgb = None;
    }

    fn render_tolerance(
        &mut self,
        tol: &acadrust::entities::Tolerance,
        transform: &Transform2D,
        stream: &mut String,
        state: &mut GraphicsState,
        r: f32,
        g: f32,
        b: f32,
        line_width_pt: f64,
    ) {
        let raw_text = tol.text.trim().trim_matches('"');
        let lines: Vec<&str> = raw_text.split(|c| c == '\n' || c == '\r').flat_map(|s| s.split("^J")).collect();
        let mut rows: Vec<Vec<String>> = Vec::new();
        for line in lines {
            let mut cells: Vec<String> = line.split("%%v").map(|s| s.trim().trim_matches('"').to_string()).collect();
            while cells.last().map_or(false, |s| s.trim().is_empty()) {
                cells.pop();
            }
            if !cells.is_empty() {
                rows.push(cells);
            }
        }
        if rows.is_empty() {
            return;
        }

        let matched_ds = self.doc.dim_styles
            .iter()
            .find(|ds| ds.name.eq_ignore_ascii_case(&tol.dimension_style_name));

        let (dimtxt, dimgap, text_style) = if let Some(ds) = matched_ds {
            let h = if ds.dimtxt > 1e-4 { ds.dimtxt } else { 2.5 };
            let g = if ds.dimgap > 1e-4 { ds.dimgap } else { h * 0.4 };
            let style = if !ds.dimtxsty.is_empty() { ds.dimtxsty.as_str() } else { "STANDARD" };
            (h, g, style)
        } else {
            (2.5, 1.0, "STANDARD")
        };

        let text_h = if tol.text_height > 1e-4 { tol.text_height } else { dimtxt };
        let gap = if tol.dimension_gap > 1e-4 { tol.dimension_gap } else { dimgap };
        let row_h = text_h + 2.0 * gap;

        let dir_len = tol.direction.x.hypot(tol.direction.y);
        let (ux_w, uy_w) = if dir_len > 1e-6 { (tol.direction.x / dir_len, tol.direction.y / dir_len) } else { (1.0, 0.0) };
        let (dx_w, dy_w) = (uy_w, -ux_w); // downward perpendicular in WCS

        let frame_stroke_w = (line_width_pt * 0.75).clamp(0.35, 1.0);

        for (r_idx, cells) in rows.iter().enumerate() {
            let mut cum_w = 0.0;
            for cell_str in cells {
                let trimmed = cell_str.trim();
                let is_sym = (trimmed.starts_with("{\\Fgdt;") || trimmed.starts_with("{\\FGDT;")) && trimmed.ends_with('}');
                let sym_char = if is_sym {
                    let inner = &trimmed[7..trimmed.len() - 1];
                    inner.chars().next().map(|c| c.to_ascii_lowercase())
                } else if trimmed.len() == 1 && "pjdnacfegtumls".contains(trimmed.to_ascii_lowercase().as_str()) {
                    trimmed.chars().next().map(|c| c.to_ascii_lowercase())
                } else {
                    None
                };

                let clean_text = trimmed
                    .replace("{\\Fgdt;m}", " (M)")
                    .replace("{\\Fgdt;l}", " (L)")
                    .replace("{\\Fgdt;s}", " (S)")
                    .replace("{\\Fgdt;%}", "Ø")
                    .replace("{\\Fgdt;c}", "Ø")
                    .replace("{\\FGDT;M}", " (M)")
                    .replace("{\\FGDT;L}", " (L)")
                    .replace("{\\FGDT;S}", " (S)")
                    .replace("{\\FGDT;%}", "Ø")
                    .replace("{\\FGDT;C}", "Ø");
                let decoded = decode_cad_text_escapes(&clean_text);

                let cell_w = if sym_char.is_some() {
                    row_h
                } else {
                    let w_est = estimate_str_width(&decoded, text_h);
                    (w_est + 2.0 * gap).max(row_h)
                };

                let x0 = tol.insertion_point.x + cum_w * ux_w + (r_idx as f64 * row_h) * dx_w;
                let y0 = tol.insertion_point.y + cum_w * uy_w + (r_idx as f64 * row_h) * dy_w;

                let p_tl = (x0, y0);
                let p_tr = (x0 + cell_w * ux_w, y0 + cell_w * uy_w);
                let p_br = (p_tr.0 + row_h * dx_w, p_tr.1 + row_h * dy_w);
                let p_bl = (p_tl.0 + row_h * dx_w, p_tl.1 + row_h * dy_w);

                let (tl_x, tl_y) = transform.apply(p_tl.0, p_tl.1);
                let (tr_x, tr_y) = transform.apply(p_tr.0, p_tr.1);
                let (br_x, br_y) = transform.apply(p_br.0, p_br.1);
                let (bl_x, bl_y) = transform.apply(p_bl.0, p_bl.1);

                // Stroke cell frame box
                Self::ensure_stroke(stream, state, r, g, b, frame_stroke_w, None);
                stream.push_str(&format!(
                    "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h S\n",
                    tl_x, tl_y, tr_x, tr_y, br_x, br_y, bl_x, bl_y
                ));

                // Draw contents
                let center_wcs = (
                    (p_tl.0 + p_tr.0 + p_br.0 + p_bl.0) * 0.25,
                    (p_tl.1 + p_tr.1 + p_br.1 + p_bl.1) * 0.25,
                );

                if let Some(ch) = sym_char {
                    // Draw vector GD&T symbol centered at center_wcs
                    let (cx_pdf, cy_pdf) = transform.apply(center_wcs.0, center_wcs.1);
                    let (ux_pdf, uy_pdf) = transform.apply_delta(ux_w, uy_w);
                    let u_len = ux_pdf.hypot(uy_pdf);
                    let (u_norm_x, u_norm_y) = if u_len > 1e-6 { (ux_pdf / u_len, uy_pdf / u_len) } else { (1.0, 0.0) };
                    let (v_norm_x, v_norm_y) = (-u_norm_y, u_norm_x); // upward in PDF
                    let s_h = text_h * transform.scale();

                    match ch {
                        'p' => {
                            // Position symbol: circle with crosshair
                            let rad = s_h * 0.38;
                            stream.push_str(&generate_circle_path(cx_pdf, cy_pdf, rad));
                            stream.push_str("S\n");
                            let arm = s_h * 0.52;
                            let x1 = cx_pdf - arm * u_norm_x;
                            let y1 = cy_pdf - arm * u_norm_y;
                            let x2 = cx_pdf + arm * u_norm_x;
                            let y2 = cy_pdf + arm * u_norm_y;
                            let y3 = cy_pdf - arm * v_norm_y;
                            let x3 = cx_pdf - arm * v_norm_x;
                            let y4 = cy_pdf + arm * v_norm_y;
                            let x4 = cx_pdf + arm * v_norm_x;
                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} m {:.2} {:.2} l S\n", x1, y1, x2, y2, x3, y3, x4, y4));
                        }
                        'j' => {
                            // Perpendicularity symbol: bottom bar and vertical stem
                            let b_arm = s_h * 0.35;
                            let bot_cx = cx_pdf - s_h * 0.35 * v_norm_x;
                            let bot_cy = cy_pdf - s_h * 0.35 * v_norm_y;
                            let x1 = bot_cx - b_arm * u_norm_x;
                            let y1 = bot_cy - b_arm * u_norm_y;
                            let x2 = bot_cx + b_arm * u_norm_x;
                            let y2 = bot_cy + b_arm * u_norm_y;
                            let top_x = cx_pdf + s_h * 0.35 * v_norm_x;
                            let top_y = cy_pdf + s_h * 0.35 * v_norm_y;
                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} m {:.2} {:.2} l S\n", x1, y1, x2, y2, bot_cx, bot_cy, top_x, top_y));
                        }
                        'd' => {
                            // Parallelism: two slanted lines //
                            let off = s_h * 0.15;
                            let sl_x = s_h * 0.18 * u_norm_x;
                            let sl_y = s_h * 0.18 * u_norm_y;
                            let h_v = s_h * 0.35;
                            for dir in [-1.0, 1.0] {
                                let c_x = cx_pdf + dir * off * u_norm_x;
                                let c_y = cy_pdf + dir * off * u_norm_y;
                                let p1x = c_x - sl_x - h_v * v_norm_x;
                                let p1y = c_y - sl_y - h_v * v_norm_y;
                                let p2x = c_x + sl_x + h_v * v_norm_x;
                                let p2y = c_y + sl_y + h_v * v_norm_y;
                                stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", p1x, p1y, p2x, p2y));
                            }
                        }
                        'n' => {
                            // Angularity: acute angle /_
                            let bot_x = cx_pdf - s_h * 0.35 * v_norm_x;
                            let bot_y = cy_pdf - s_h * 0.35 * v_norm_y;
                            let corner_x = bot_x - s_h * 0.35 * u_norm_x;
                            let corner_y = bot_y - s_h * 0.35 * u_norm_y;
                            let r_x = bot_x + s_h * 0.35 * u_norm_x;
                            let r_y = bot_y + s_h * 0.35 * u_norm_y;
                            let top_x = corner_x + s_h * 0.5 * u_norm_x + s_h * 0.7 * v_norm_x;
                            let top_y = corner_y + s_h * 0.5 * u_norm_y + s_h * 0.7 * v_norm_y;
                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l S\n", top_x, top_y, corner_x, corner_y, r_x, r_y));
                        }
                        'a' => {
                            // Straightness: horizontal line -
                            let arm = s_h * 0.38;
                            let x1 = cx_pdf - arm * u_norm_x;
                            let y1 = cy_pdf - arm * u_norm_y;
                            let x2 = cx_pdf + arm * u_norm_x;
                            let y2 = cy_pdf + arm * u_norm_y;
                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", x1, y1, x2, y2));
                        }
                        'c' | 'f' => {
                            // Flatness: parallelogram
                            let sl = s_h * 0.18;
                            let w_h = s_h * 0.32;
                            let h_v = s_h * 0.22;
                            let p1x = cx_pdf - w_h * u_norm_x - sl * u_norm_x - h_v * v_norm_x;
                            let p1y = cy_pdf - w_h * u_norm_y - sl * u_norm_y - h_v * v_norm_y;
                            let p2x = cx_pdf + w_h * u_norm_x - sl * u_norm_x - h_v * v_norm_x;
                            let p2y = cy_pdf + w_h * u_norm_y - sl * u_norm_y - h_v * v_norm_y;
                            let p3x = cx_pdf + w_h * u_norm_x + sl * u_norm_x + h_v * v_norm_x;
                            let p3y = cy_pdf + w_h * u_norm_y + sl * u_norm_y + h_v * v_norm_y;
                            let p4x = cx_pdf - w_h * u_norm_x + sl * u_norm_x + h_v * v_norm_x;
                            let p4y = cy_pdf - w_h * u_norm_y + sl * u_norm_y + h_v * v_norm_y;
                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h S\n", p1x, p1y, p2x, p2y, p3x, p3y, p4x, p4y));
                        }
                        'e' => {
                            // Circularity: circle
                            let rad = s_h * 0.35;
                            stream.push_str(&generate_circle_path(cx_pdf, cy_pdf, rad));
                            stream.push_str("S\n");
                        }
                        _ => {
                            let rad = s_h * 0.35;
                            stream.push_str(&generate_circle_path(cx_pdf, cy_pdf, rad));
                            stream.push_str("S\n");
                        }
                    }
                } else if !decoded.is_empty() {
                    let rot_rad = tol.direction.y.atan2(tol.direction.x);
                    self.render_single_line_text(
                        &decoded,
                        center_wcs,
                        None,
                        text_h,
                        rot_rad,
                        1.0,
                        0.0,
                        text_style,
                        acadrust::entities::text::TextHorizontalAlignment::Center,
                        acadrust::entities::text::TextVerticalAlignment::Middle,
                        0,
                        tol.normal,
                        transform,
                        stream,
                        state,
                        r,
                        g,
                        b,
                    );
                }

                cum_w += cell_w;
            }
        }
    }

    fn render_single_line_text(
        &mut self,
        value: &str,
        p1: (f64, f64),
        p2: Option<(f64, f64)>,
        height: f64,
        rotation: f64,
        width_factor: f64,
        oblique_angle: f64,
        style: &str,
        h_align: acadrust::entities::text::TextHorizontalAlignment,
        v_align: acadrust::entities::text::TextVerticalAlignment,
        generation_flags: i16,
        normal: Vector3,
        transform: &Transform2D,
        stream: &mut String,
        state: &mut GraphicsState,
        r: f32,
        g: f32,
        b: f32,
    ) {
        let decoded = decode_cad_text_escapes(value);
        if decoded.is_empty() {
            return;
        }
        Self::ensure_fill(stream, state, r, g, b);

        let m = Matrix3::arbitrary_axis(normal);
        let p1_w = m.transform_point(Vector3::new(p1.0, p1.1, 0.0));
        let p1_trans = (p1_w.x, p1_w.y);
        let p2_trans = p2.map(|pt| {
            let p2_w = m.transform_point(Vector3::new(pt.0, pt.1, 0.0));
            (p2_w.x, p2_w.y)
        });

        let mut eff_height = height;
        let mut eff_wf = if width_factor > 0.01 && (width_factor - 1.0).abs() > 0.01 {
            width_factor
        } else {
            1.0
        };
        let mut eff_rot = rotation;
        let mut eff_h_align = h_align;
        let mut eff_v_align = v_align;
        let mut p_align = p2_trans;

        let category = self.resolve_font_category(style);
        let cap_ratio = category.cap_height_ratio();
        let desc_ratio = category.descender_ratio();

        use acadrust::entities::text::{TextHorizontalAlignment, TextVerticalAlignment};

        match h_align {
            TextHorizontalAlignment::Aligned if p2_trans.is_some() => {
                let (x2, y2) = p2_trans.unwrap();
                let dx = x2 - p1_trans.0;
                let dy = y2 - p1_trans.1;
                let d = dx.hypot(dy);
                if d > 1e-4 {
                    eff_rot = dy.atan2(dx);
                    let w0 = estimate_str_width_category(&decoded, 1.0, category) * eff_wf;
                    if w0 > 1e-4 {
                        eff_height = (d * cap_ratio) / w0;
                    }
                    eff_h_align = TextHorizontalAlignment::Left;
                    eff_v_align = TextVerticalAlignment::Baseline;
                    p_align = None;
                }
            }
            TextHorizontalAlignment::Fit if p2_trans.is_some() => {
                let (x2, y2) = p2_trans.unwrap();
                let dx = x2 - p1_trans.0;
                let dy = y2 - p1_trans.1;
                let d = dx.hypot(dy);
                if d > 1e-4 {
                    eff_rot = dy.atan2(dx);
                    let w0 = estimate_str_width_category(&decoded, eff_height, category);
                    if w0 > 1e-4 {
                        eff_wf = (d * cap_ratio) / w0;
                    }
                    eff_h_align = TextHorizontalAlignment::Left;
                    eff_v_align = TextVerticalAlignment::Baseline;
                    p_align = None;
                }
            }
            _ => {}
        }

        let font_size = (eff_height * transform.scale() / cap_ratio).max(0.05);

        let ref_pt = if (eff_h_align != TextHorizontalAlignment::Left
            || eff_v_align != TextVerticalAlignment::Baseline)
            && p_align.is_some()
        {
            p_align.unwrap()
        } else {
            p1_trans
        };

        let text_w_pt = estimate_str_width_category(&decoded, font_size, category) * eff_wf;

        let delta_u = match eff_h_align {
            TextHorizontalAlignment::Center | TextHorizontalAlignment::Middle => -text_w_pt / 2.0,
            TextHorizontalAlignment::Right => -text_w_pt,
            _ => 0.0,
        };

        let delta_v = match eff_v_align {
            TextVerticalAlignment::Bottom => font_size * desc_ratio,
            TextVerticalAlignment::Middle => -0.5 * (eff_height * transform.scale()),
            TextVerticalAlignment::Top => -(eff_height * transform.scale()),
            TextVerticalAlignment::Baseline => {
                if eff_h_align == TextHorizontalAlignment::Middle {
                    -0.5 * (eff_height * transform.scale())
                } else {
                    0.0
                }
            }
        };

        let is_backward = (generation_flags & 2) != 0;
        let is_upside_down = (generation_flags & 4) != 0;

        let eff_du = if is_backward { -delta_u } else { delta_u };
        let eff_dv = if is_upside_down { -delta_v } else { delta_v };

        let (u_wcs, v_wcs) = if (h_align == TextHorizontalAlignment::Aligned || h_align == TextHorizontalAlignment::Fit) && p2.is_some() {
            (
                Vector3::new(eff_rot.cos(), eff_rot.sin(), 0.0),
                Vector3::new(-eff_rot.sin(), eff_rot.cos(), 0.0),
            )
        } else {
            (
                m.transform_point(Vector3::new(eff_rot.cos(), eff_rot.sin(), 0.0)),
                m.transform_point(Vector3::new(-eff_rot.sin(), eff_rot.cos(), 0.0)),
            )
        };

        let tx_u = transform.a * u_wcs.x + transform.c * u_wcs.y;
        let ty_u = transform.b * u_wcs.x + transform.d * u_wcs.y;
        let len_u = tx_u.hypot(ty_u).max(1e-6);
        let u_hat = (tx_u / len_u, ty_u / len_u);

        let tx_v = transform.a * v_wcs.x + transform.c * v_wcs.y;
        let ty_v = transform.b * v_wcs.x + transform.d * v_wcs.y;
        let len_v = tx_v.hypot(ty_v).max(1e-6);
        let v_hat = (tx_v / len_v, ty_v / len_v);

        let (p_ref_x, p_ref_y) = transform.apply(ref_pt.0, ref_pt.1);
        let p_origin_x = p_ref_x + eff_du * u_hat.0 + eff_dv * v_hat.0;
        let p_origin_y = p_ref_y + eff_du * u_hat.1 + eff_dv * v_hat.1;

        let flip_x = if is_backward { -1.0 } else { 1.0 };
        let flip_y = if is_upside_down { -1.0 } else { 1.0 };

        let a_b = (u_hat.0 * flip_x, u_hat.1 * flip_x);
        let mut c_d = (v_hat.0 * flip_y, v_hat.1 * flip_y);

        if oblique_angle.abs() > 1e-4 {
            let tan_obl = oblique_angle.tan();
            c_d.0 += tan_obl * a_b.0;
            c_d.1 += tan_obl * a_b.1;
        }

        let is_bold = style.to_lowercase().contains("bold") || style.to_lowercase().contains("black") || self.doc.text_styles.iter().any(|s| {
            s.name.eq_ignore_ascii_case(style) && (
                s.name.to_lowercase().contains("bold")
                || s.name.to_lowercase().contains("black")
                || s.font_file.to_lowercase().contains("bold")
                || s.font_file.to_lowercase().contains("black")
                || s.font_file.to_lowercase().contains("blk")
                || s.true_type_font.to_lowercase().contains("bold")
                || s.true_type_font.to_lowercase().contains("black")
            )
        });
        let is_italic = style.to_lowercase().contains("italic") || style.to_lowercase().contains("oblique") || oblique_angle.abs() > 0.05 || self.doc.text_styles.iter().any(|s| {
            s.name.eq_ignore_ascii_case(style) && (
                s.name.to_lowercase().contains("italic")
                || s.name.to_lowercase().contains("oblique")
                || s.font_file.to_lowercase().contains("italic")
                || s.font_file.to_lowercase().contains("oblique")
                || s.true_type_font.to_lowercase().contains("italic")
                || s.true_type_font.to_lowercase().contains("oblique")
            )
        });
        let font_name = self.select_font_name(category, is_bold, is_italic);
        let escaped = encode_pdf_string(&decoded);

        if (eff_wf - 1.0).abs() > 0.01 {
            let tz = (eff_wf * 100.0).clamp(5.0, 1000.0);
            stream.push_str(&format!(
                "BT\n/{} {:.2} Tf\n{:.1} Tz\n{:.4} {:.4} {:.4} {:.4} {:.2} {:.2} Tm\n({}) Tj\n100 Tz\nET\n",
                font_name, font_size,
                tz,
                a_b.0, a_b.1, c_d.0, c_d.1, p_origin_x, p_origin_y,
                escaped
            ));
        } else {
            stream.push_str(&format!(
                "BT\n/{} {:.2} Tf\n{:.4} {:.4} {:.4} {:.4} {:.2} {:.2} Tm\n({}) Tj\nET\n",
                font_name, font_size,
                a_b.0, a_b.1, c_d.0, c_d.1, p_origin_x, p_origin_y,
                escaped
            ));
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
        parent_layer: Option<&str>,
        parent_color: Option<&Color>,
    ) {
        if depth > 32 {
            return;
        }

        let dyn_ent = entity.as_entity();
        if dyn_ent.is_invisible() {
            return;
        }

        let raw_layer = dyn_ent.layer();
        // Dynamic Layer 0 adoption in AutoCAD blocks:
        // Entities on layer "0" inside a block definition adopt the layer of the INSERT entity
        let layer_name = if raw_layer == "0" {
            parent_layer.unwrap_or(raw_layer)
        } else {
            raw_layer
        };

        if layer_name.eq_ignore_ascii_case("defpoints") {
            return;
        }

        if let Some(layer) = self.doc.layers.iter().find(|l| l.name.eq_ignore_ascii_case(layer_name)) {
            if !layer.is_plottable || layer.is_frozen() || layer.is_off() {
                return;
            }
            if let Some(ref frozen) = self.active_frozen_layers {
                if frozen.contains(&layer.handle) {
                    return;
                }
            }
        }

        let layer_color_opt = self.layer_colors.get(&layer_name.to_ascii_lowercase()).cloned();
        let layer_color = layer_color_opt.as_ref();

        let entity_color = dyn_ent.color();
        let rgb = resolve_color(&entity_color, layer_color, parent_color, self.default_draw_rgb);
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

        let ltype_name = entity.common().linetype.as_str();
        let effective_ltype = if ltype_name.is_empty() || ltype_name.eq_ignore_ascii_case("bylayer") {
            self.layer_linetypes.get(&layer_name.to_ascii_lowercase()).map(|s| s.as_str()).unwrap_or("CONTINUOUS")
        } else {
            ltype_name
        };

        let global_ltscale = if self.doc.header.linetype_scale > 0.0 { self.doc.header.linetype_scale } else { 1.0 };
        let ent_ltscale = if entity.common().linetype_scale > 0.0 { entity.common().linetype_scale } else { 1.0 };
        let combined_ltscale = global_ltscale * ent_ltscale;

        let dash_pattern = if let Some(lt) = self.doc.line_types.iter().find(|l| l.name.eq_ignore_ascii_case(effective_ltype)) {
            if !lt.elements.is_empty() {
                LineDashPattern::from_elements(&lt.elements, (transform.scale() * combined_ltscale) as f32)
            } else {
                LineDashPattern::from_name(
                    effective_ltype,
                    combined_ltscale.max(0.5) as f32,
                )
            }
        } else {
            LineDashPattern::from_name(
                effective_ltype,
                combined_ltscale.max(0.5) as f32,
            )
        };
        let dash_slice = if dash_pattern.is_solid() {
            None
        } else {
            Some(dash_pattern.dash_array.as_slice())
        };

        match entity {
            EntityType::Line(line) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                let (p1x, p1y) = transform.apply(line.start.x, line.start.y);
                let (p2x, p2y) = transform.apply(line.end.x, line.end.y);
                stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", p1x, p1y, p2x, p2y));
            }
            EntityType::Circle(circle) => {
                let r_rad = circle.radius * transform.scale();
                if r_rad < 0.15 {
                    return;
                }
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                let center = circle.center_wcs();
                let (cx, cy) = transform.apply(center.x, center.y);
                stream.push_str(&generate_circle_path(cx, cy, r_rad));
                stream.push_str("S\n");
            }
            EntityType::Arc(arc) => {
                let r_rad = arc.radius * transform.scale();
                if r_rad < 0.15 && (arc.arc_length() * transform.scale()) < 0.25 {
                    return;
                }
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                let center_wcs = arc.center_wcs();
                let start_wcs = arc.start_point_wcs();
                let end_wcs = arc.end_point_wcs();

                let (cx, cy) = transform.apply(center_wcs.x, center_wcs.y);
                let (sp_x, sp_y) = transform.apply(start_wcs.x, start_wcs.y);
                let (ep_x, ep_y) = transform.apply(end_wcs.x, end_wcs.y);

                let start_rad = (sp_y - cy).atan2(sp_x - cx);
                let end_rad = (ep_y - cy).atan2(ep_x - cx);
                let ccw = (arc.normal.z >= 0.0) ^ transform.is_reflected();

                stream.push_str(&generate_arc_path_with_mode(
                    cx, cy, r_rad, start_rad, end_rad, true, ccw,
                ));
                stream.push_str("S\n");
            }
            EntityType::Ellipse(ellipse) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
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
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                let first = &lwpoly.vertices[0];
                let (p0_x, p0_y) = transform.apply(first.location.x, first.location.y);
                stream.push_str(&format!("{:.2} {:.2} m\n", p0_x, p0_y));

                for i in 0..lwpoly.vertices.len() - 1 {
                    let v1 = &lwpoly.vertices[i];
                    let v2 = &lwpoly.vertices[i + 1];

                    let (p1x, p1y) = transform.apply(v1.location.x, v1.location.y);
                    let (p2x, p2y) = transform.apply(v2.location.x, v2.location.y);

                    if v1.bulge.abs() > 1e-6 {
                        let eff_bulge = if transform.is_reflected() { -v1.bulge } else { v1.bulge };
                        stream.push_str(&generate_bulge_segment(
                            Point2D::new(p1x, p1y),
                            Point2D::new(p2x, p2y),
                            eff_bulge,
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
                        let eff_bulge = if transform.is_reflected() { -last.bulge } else { last.bulge };
                        stream.push_str(&generate_bulge_segment(
                            Point2D::new(p1x, p1y),
                            Point2D::new(p2x, p2y),
                            eff_bulge,
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
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
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
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                let first = &poly.vertices[0];
                let (p0_x, p0_y) = transform.apply(first.location.x, first.location.y);
                stream.push_str(&format!("{:.2} {:.2} m\n", p0_x, p0_y));

                for i in 0..poly.vertices.len() - 1 {
                    let v1 = &poly.vertices[i];
                    let v2 = &poly.vertices[i + 1];

                    let (p1x, p1y) = transform.apply(v1.location.x, v1.location.y);
                    let (p2x, p2y) = transform.apply(v2.location.x, v2.location.y);

                    if v1.bulge.abs() > 1e-6 {
                        let eff_bulge = if transform.is_reflected() { -v1.bulge } else { v1.bulge };
                        stream.push_str(&generate_bulge_segment(
                            Point2D::new(p1x, p1y),
                            Point2D::new(p2x, p2y),
                            eff_bulge,
                        ));
                    } else {
                        stream.push_str(&format!("{:.2} {:.2} l\n", p2x, p2y));
                    }
                }

                if poly.is_closed() && poly.vertices.len() > 1 {
                    let last = &poly.vertices[poly.vertices.len() - 1];
                    let first = &poly.vertices[0];

                    let (p1x, p1y) = transform.apply(last.location.x, last.location.y);
                    let (p2x, p2y) = transform.apply(first.location.x, first.location.y);

                    if last.bulge.abs() > 1e-6 {
                        let eff_bulge = if transform.is_reflected() { -last.bulge } else { last.bulge };
                        stream.push_str(&generate_bulge_segment(
                            Point2D::new(p1x, p1y),
                            Point2D::new(p2x, p2y),
                            eff_bulge,
                        ));
                    }
                    stream.push_str("h ");
                }
                stream.push_str("S\n");
            }
            EntityType::Point(pt) => {
                let mode = self.doc.header.point_display_mode;
                if mode != 1 {
                    Self::ensure_stroke(stream, state, r, g, b, line_width_pt, None);
                    let (px, py) = transform.apply(pt.location.x, pt.location.y);
                    let pd_size = self.doc.header.point_display_size;
                    let size = if pd_size > 0.0 {
                        (pd_size * transform.scale()).max(1.0)
                    } else if pd_size < 0.0 {
                        (pd_size.abs() * 0.01 * 600.0).max(1.0)
                    } else {
                        (line_width_pt * 2.0).clamp(1.0, 5.0)
                    };
                    let rad = size * 0.5;
                    let base = mode & 31;
                    match base {
                        0 => {
                            if (mode & 32) == 0 && (mode & 64) == 0 {
                                Self::ensure_fill(stream, state, r, g, b);
                                let dot_r = (line_width_pt * 0.75).clamp(0.75, 2.0);
                                let k = 0.552284749831 * dot_r;
                                stream.push_str(&format!(
                                    "{:.2} {:.2} m {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c h f\n",
                                    px + dot_r, py,
                                    px + dot_r, py + k, px + k, py + dot_r, px, py + dot_r,
                                    px - k, py + dot_r, px - dot_r, py + k, px - dot_r, py,
                                    px - dot_r, py - k, px - k, py - dot_r, px, py - dot_r,
                                    px + k, py - dot_r, px + dot_r, py - k, px + dot_r, py
                                ));
                            }
                        }
                        2 => {
                            stream.push_str(&format!(
                                "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} m {:.2} {:.2} l S\n",
                                px - rad, py, px + rad, py,
                                px, py - rad, px, py + rad
                            ));
                        }
                        3 => {
                            let rd = rad * 0.7071067811865476;
                            stream.push_str(&format!(
                                "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} m {:.2} {:.2} l S\n",
                                px - rd, py - rd, px + rd, py + rd,
                                px - rd, py + rd, px + rd, py - rd
                            ));
                        }
                        4 => {
                            stream.push_str(&format!(
                                "{:.2} {:.2} m {:.2} {:.2} l S\n",
                                px, py - rad, px, py + rad
                            ));
                        }
                        _ => {}
                    }
                    if (mode & 32) != 0 {
                        let k = 0.552284749831 * rad;
                        stream.push_str(&format!(
                            "{:.2} {:.2} m {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c h S\n",
                            px + rad, py,
                            px + rad, py + k, px + k, py + rad, px, py + rad,
                            px - k, py + rad, px - rad, py + k, px - rad, py,
                            px - rad, py - k, px - k, py - rad, px, py - rad,
                            px + k, py - rad, px + rad, py - k, px + rad, py
                        ));
                    }
                    if (mode & 64) != 0 {
                        stream.push_str(&format!(
                            "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h S\n",
                            px - rad, py - rad,
                            px + rad, py - rad,
                            px + rad, py + rad,
                            px - rad, py + rad
                        ));
                    }
                }
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
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
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
                let p1 = (txt.insertion_point.x, txt.insertion_point.y);
                let p2 = txt.alignment_point.as_ref().map(|p| (p.x, p.y));
                self.render_single_line_text(
                    &txt.value,
                    p1,
                    p2,
                    txt.height,
                    txt.rotation,
                    txt.width_factor,
                    txt.oblique_angle,
                    &txt.style,
                    txt.horizontal_alignment,
                    txt.vertical_alignment,
                    txt.generation_flags,
                    txt.normal,
                    transform,
                    stream,
                    state,
                    r, g, b,
                );
            }
            EntityType::AttributeDefinition(attdef) => {
                if depth > 0 || attdef.flags.invisible || attdef.default_value.trim().is_empty() {
                    return;
                }
                let val = attdef.default_value.trim();
                let p1 = (attdef.insertion_point.x, attdef.insertion_point.y);
                let p2 = Some((attdef.alignment_point.x, attdef.alignment_point.y));
                self.render_single_line_text(
                    val,
                    p1,
                    p2,
                    attdef.height,
                    attdef.rotation,
                    attdef.width_factor,
                    attdef.oblique_angle,
                    &attdef.text_style,
                    Self::map_attrib_h_align(attdef.horizontal_alignment),
                    Self::map_attrib_v_align(attdef.vertical_alignment),
                    attdef.text_generation_flags,
                    attdef.normal,
                    transform,
                    stream,
                    state,
                    r, g, b,
                );
            }
            EntityType::AttributeEntity(attrib) => {
                if attrib.flags.invisible || attrib.value.trim().is_empty() {
                    return;
                }
                let val = attrib.value.trim();
                let p1 = (attrib.insertion_point.x, attrib.insertion_point.y);
                let p2 = Some((attrib.alignment_point.x, attrib.alignment_point.y));
                self.render_single_line_text(
                    val,
                    p1,
                    p2,
                    attrib.height,
                    attrib.rotation,
                    attrib.width_factor,
                    attrib.oblique_angle,
                    &attrib.text_style,
                    Self::map_attrib_h_align(attrib.horizontal_alignment),
                    Self::map_attrib_v_align(attrib.vertical_alignment),
                    attrib.text_generation_flags,
                    attrib.normal,
                    transform,
                    stream,
                    state,
                    r, g, b,
                );
            }
            EntityType::MText(mtext) => {
                Self::ensure_fill(stream, state, r, g, b);
                let (px, py) = transform.apply(mtext.insertion_point.x, mtext.insertion_point.y);

                let mtext_val = self.resolve_mtext_value(mtext);

                let category = if mtext_val.contains("\\fArial")
                    || mtext_val.contains("\\fHelvetica")
                    || mtext_val.contains("\\fCalibri")
                    || mtext_val.contains("\\fTahoma")
                    || mtext_val.contains("\\fVerdana")
                    || mtext_val.contains("\\fSans")
                {
                    FontCategory::SansSerif
                } else if mtext_val.contains("\\fTimes")
                    || mtext_val.contains("\\fGeorgia")
                    || mtext_val.contains("\\fCentury")
                    || mtext_val.contains("\\fGaramond")
                {
                    FontCategory::Serif
                } else if mtext_val.contains("\\fCourier")
                    || mtext_val.contains("\\fConsolas")
                    || mtext_val.contains("\\fMono")
                {
                    FontCategory::Monospace
                } else {
                    self.resolve_font_category(&mtext.style)
                };
                let _cap_ratio = category.cap_height_ratio();
                let base_font_size = (mtext.height * transform.scale()).max(0.05);

                let is_shx = if let Some(style) = self.doc.text_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&mtext.style)) {
                    let f = style.font_file.to_lowercase();
                    let t = style.true_type_font.to_lowercase();
                    t.is_empty() && (f.ends_with(".shx") || f == "txt" || f == "simplex" || f == "romans" || f == "complex" || (!f.ends_with(".ttf") && !f.ends_with(".otf")))
                } else {
                    true
                };
                let mut details = parse_mtext_details_with_shx(&mtext_val, mtext.rectangle_width, mtext.height, is_shx);

                let style_is_bold = mtext.style.to_lowercase().contains("bold") || mtext.style.to_lowercase().contains("black") || self.doc.text_styles.iter().any(|s| {
                    s.name.eq_ignore_ascii_case(&mtext.style) && (
                        s.name.to_lowercase().contains("bold")
                        || s.name.to_lowercase().contains("black")
                        || s.font_file.to_lowercase().contains("bold")
                        || s.font_file.to_lowercase().contains("black")
                        || s.font_file.to_lowercase().contains("blk")
                        || s.true_type_font.to_lowercase().contains("bold")
                        || s.true_type_font.to_lowercase().contains("black")
                    )
                });
                let style_is_italic = mtext.style.to_lowercase().contains("italic") || mtext.style.to_lowercase().contains("oblique") || self.doc.text_styles.iter().any(|s| {
                    s.name.eq_ignore_ascii_case(&mtext.style) && (
                        s.name.to_lowercase().contains("italic")
                        || s.name.to_lowercase().contains("oblique")
                        || s.font_file.to_lowercase().contains("italic")
                        || s.font_file.to_lowercase().contains("oblique")
                        || s.true_type_font.to_lowercase().contains("italic")
                        || s.true_type_font.to_lowercase().contains("oblique")
                    )
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

                let sign_z = if mtext.normal.z.is_sign_negative() { -1.0 } else { 1.0 };
                let u_wcs = Vector3::new(mtext.rotation.cos(), mtext.rotation.sin(), 0.0);
                let v_wcs = Vector3::new(-mtext.rotation.sin() * sign_z, mtext.rotation.cos() * sign_z, 0.0);

                let tx_x = transform.a * u_wcs.x + transform.c * u_wcs.y;
                let tx_y = transform.b * u_wcs.x + transform.d * u_wcs.y;
                let ty_x = transform.a * v_wcs.x + transform.c * v_wcs.y;
                let ty_y = transform.b * v_wcs.x + transform.d * v_wcs.y;

                let len_x = tx_x.hypot(tx_y).max(1e-6);
                let len_y = ty_x.hypot(ty_y).max(1e-6);

                let (cos, sin) = (tx_x / len_x, tx_y / len_x);
                let (c_vec, d_vec) = (ty_x / len_y, ty_y / len_y);

                if mtext.drawing_direction == acadrust::entities::mtext::DrawingDirection::TopToBottom {
                    let spacing_factor = if mtext.line_spacing_factor > 0.05 { mtext.line_spacing_factor } else { 1.0 };
                    let col_spacing = base_font_size * 1.30 * spacing_factor;
                    let char_advance = base_font_size * 1.25;

                    for (col_idx, line_info) in details.lines.iter().enumerate() {
                        let col_x = (col_idx as f64) * col_spacing;
                        let mut char_idx = 0;
                        for c in line_info.text.chars() {
                            if c == ' ' {
                                char_idx += 1;
                                continue;
                            }
                            let char_y = -(char_idx as f64) * char_advance - base_font_size * 0.72;
                            let c_px = px + cos * col_x + c_vec * char_y;
                            let c_py = py + sin * col_x + d_vec * char_y;

                            let font_name = self.select_font_name(category, line_info.is_bold, line_info.is_italic);
                            let s_escaped = encode_pdf_string(&c.to_string());
                            stream.push_str(&format!(
                                "BT\n/{} {:.2} Tf\n{:.4} {:.4} {:.4} {:.4} {:.2} {:.2} Tm\n({}) Tj\nET\n",
                                font_name, base_font_size,
                                cos, sin, c_vec, d_vec, c_px, c_py,
                                s_escaped
                            ));
                            char_idx += 1;
                        }
                    }
                    return;
                }

                let attach_val = mtext.attachment_point as u8;
                let is_center_h = attach_val == 2 || attach_val == 5 || attach_val == 8;
                let is_right_h = attach_val == 3 || attach_val == 6 || attach_val == 9;

                let num_lines = details.lines.len().max(1);
                let line_font_sizes: Vec<f64> = details.lines.iter().map(|l| {
                    (base_font_size * l.height_scale).max(0.05)
                }).collect();

                let spacing_factor = if mtext.line_spacing_factor > 0.05 { mtext.line_spacing_factor } else { 1.0 };
                let line_spacings: Vec<f64> = line_font_sizes.iter().map(|&sz| sz * (5.0 / 3.0) * spacing_factor).collect();
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
                    -cap_height_0
                };

                let has_bg_fill = (mtext.background_fill_flags & 0x01) != 0 || (mtext.background_fill_flags & 0x02) != 0;
                let has_text_frame = (mtext.background_fill_flags & 0x10) != 0;

                if has_bg_fill || has_text_frame {
                    let rect_w_pt = (mtext.rectangle_width * transform.scale()).max(0.0);
                    let max_content_w = details.lines.iter().map(|l| {
                        let sz = base_font_size * l.height_scale;
                        estimate_str_width(&l.text, sz) * l.width_factor
                    }).fold(0.0, f64::max);
                    let box_w = if rect_w_pt > 0.0 { rect_w_pt } else { max_content_w };

                    let (box_min_x, box_max_x) = if is_center_h {
                        (-box_w / 2.0, box_w / 2.0)
                    } else if is_right_h {
                        (-box_w, 0.0)
                    } else {
                        (0.0, box_w)
                    };

                    let (box_min_y, box_max_y) = if attach_val >= 4 && attach_val <= 6 {
                        (-total_block_h / 2.0, total_block_h / 2.0)
                    } else if attach_val >= 7 {
                        (0.0, total_block_h)
                    } else {
                        (-total_block_h, 0.0)
                    };

                    let scale_fac = if mtext.background_scale > 1.0 { mtext.background_scale } else { 1.5 };
                    let border = (scale_fac - 1.0) * base_font_size * 0.5;

                    let x0 = box_min_x - border;
                    let x1 = box_max_x + border;
                    let y0 = box_min_y - border;
                    let y1 = box_max_y + border;

                    let p0 = (px + cos * x0 + c_vec * y0, py + sin * x0 + d_vec * y0);
                    let p1 = (px + cos * x1 + c_vec * y0, py + sin * x1 + d_vec * y0);
                    let p2 = (px + cos * x1 + c_vec * y1, py + sin * x1 + d_vec * y1);
                    let p3 = (px + cos * x0 + c_vec * y1, py + sin * x0 + d_vec * y1);

                    if has_bg_fill {
                        let bg_rgb = match mtext.background_color {
                            Color::Index(idx) => Some(aci_to_rgb(idx)),
                            Color::Rgb { r, g, b } => Some((r, g, b)),
                            _ => {
                                if (mtext.background_fill_flags & 0x02) != 0 {
                                    Some((255, 255, 255))
                                } else {
                                    None
                                }
                            }
                        };
                        if let Some(bg) = bg_rgb {
                            let (br_r, br_g, br_b) = rgb_to_pdf(bg);
                            Self::ensure_fill(stream, state, br_r, br_g, br_b);
                            stream.push_str(&format!(
                                "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h f\n",
                                p0.0, p0.1, p1.0, p1.1, p2.0, p2.1, p3.0, p3.1
                            ));
                        }
                    }

                    if has_text_frame {
                        Self::ensure_stroke(stream, state, r, g, b, 0.5, None);
                        stream.push_str(&format!(
                            "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h S\n",
                            p0.0, p0.1, p1.0, p1.1, p2.0, p2.1, p3.0, p3.1
                        ));
                    }

                    Self::ensure_fill(stream, state, r, g, b);
                }

                let mut cum_y = 0.0;
                for (idx, line_info) in details.lines.iter().enumerate() {
                    let font_size = line_font_sizes[idx];
                    if idx > 0 {
                        cum_y += line_spacings[idx - 1];
                    }
                    if line_info.text.is_empty() {
                        continue;
                    }
                    let font_name = self.select_font_name(category, line_info.is_bold, line_info.is_italic);

                    let wf = line_info.width_factor;
                    let tf = line_info.tracking_factor;
                    let mut char_space = if line_info.spans.is_empty() && (tf - 1.0).abs() > 0.01 {
                        (tf - 1.0) * font_size * 0.55
                    } else {
                        0.0
                    };

                    let num_chars = line_info.text.chars().count();
                    let tracking_total = char_space * (num_chars.saturating_sub(1) as f64);
                    let base_w = estimate_str_width_category(&line_info.text, font_size, category) * wf;

                    let mut line_w_pt: f64 = if !line_info.spans.is_empty() {
                        line_info.spans.iter().map(|s| {
                            let sz = (base_font_size * s.height_scale).max(0.05);
                            let tf = s.tracking_factor;
                            let cs = if (tf - 1.0).abs() > 0.01 { (tf - 1.0) * sz * 0.55 } else { 0.0 };
                            let nc = s.text.chars().count();
                            let tt = cs * (nc.saturating_sub(1) as f64);
                            estimate_str_width_category(&s.text, sz, category) * s.width_factor + tt
                        }).sum()
                    } else {
                        base_w + tracking_total
                    };

                    let natural_w = line_w_pt;
                    let indent_pt = line_info.indent * transform.scale();
                    let rect_w_pt = (mtext.rectangle_width * transform.scale()).max(0.0);

                    if line_info.alignment == MTextAlignment::Distributed && rect_w_pt > 0.0 && num_chars > 1 {
                        let available_w = (rect_w_pt - indent_pt).max(natural_w);
                        if available_w > natural_w {
                            char_space = (available_w - natural_w) / (num_chars - 1) as f64;
                            line_w_pt = available_w;
                        }
                    }

                    let eff_align = match line_info.alignment {
                        MTextAlignment::Distributed => MTextAlignment::Distributed,
                        MTextAlignment::Center => MTextAlignment::Center,
                        MTextAlignment::Right => MTextAlignment::Right,
                        _ => {
                            if is_center_h {
                                MTextAlignment::Center
                            } else if is_right_h {
                                MTextAlignment::Right
                            } else {
                                MTextAlignment::Left
                            }
                        }
                    };

                    let h_offset = match eff_align {
                        MTextAlignment::Center => {
                            if is_center_h {
                                -line_w_pt / 2.0 + indent_pt
                            } else if is_right_h {
                                if rect_w_pt > 0.0 {
                                    -rect_w_pt / 2.0 - line_w_pt / 2.0 + indent_pt
                                } else {
                                    -line_w_pt / 2.0 + indent_pt
                                }
                            } else {
                                if rect_w_pt > 0.0 {
                                    (rect_w_pt - line_w_pt) / 2.0 + indent_pt
                                } else {
                                    -line_w_pt / 2.0 + indent_pt
                                }
                            }
                        }
                        MTextAlignment::Right => {
                            if is_right_h {
                                -line_w_pt + indent_pt
                            } else if is_center_h {
                                if rect_w_pt > 0.0 {
                                    rect_w_pt / 2.0 - line_w_pt + indent_pt
                                } else {
                                    -line_w_pt + indent_pt
                                }
                            } else {
                                if rect_w_pt > 0.0 {
                                    rect_w_pt - line_w_pt + indent_pt
                                } else {
                                    -line_w_pt + indent_pt
                                }
                            }
                        }
                        MTextAlignment::Distributed => {
                            if is_center_h {
                                -line_w_pt / 2.0 + indent_pt
                            } else if is_right_h {
                                -line_w_pt + indent_pt
                            } else {
                                indent_pt
                            }
                        }
                        _ => {
                            if is_center_h {
                                if rect_w_pt > 0.0 {
                                    -rect_w_pt / 2.0 + indent_pt
                                } else {
                                    -line_w_pt / 2.0 + indent_pt
                                }
                            } else if is_right_h {
                                if rect_w_pt > 0.0 {
                                    -rect_w_pt + indent_pt
                                } else {
                                    -line_w_pt + indent_pt
                                }
                            } else {
                                indent_pt
                            }
                        }
                    };

                    let line_y_local = v_offset_line0 - cum_y;
                    let line_px = px + cos * h_offset + c_vec * line_y_local;
                    let line_py = py + sin * h_offset + d_vec * line_y_local;

                    if line_info.spans.is_empty() {
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
                            cos, sin, c_vec, d_vec, line_px, line_py,
                            escaped,
                            reset_ops
                        ));
                    } else {
                        let mut cum_x = 0.0;
                        for span in &line_info.spans {
                            if span.text.is_empty() {
                                continue;
                            }
                            let span_sz = (base_font_size * span.height_scale).max(0.05);
                            let span_font = self.select_font_name(category, span.is_bold, span.is_italic);

                            let (pdf_r, pdf_g, pdf_b) = if let Some(rgb) = span.color_rgb {
                                rgb_to_pdf(rgb)
                            } else if let Some(aci) = span.color_aci {
                                rgb_to_pdf(aci_to_rgb(aci))
                            } else {
                                (r, g, b)
                            };

                            let span_wf = span.width_factor;
                            let span_tf = span.tracking_factor;
                            let span_tracking = if (span_tf - 1.0).abs() > 0.01 {
                                (span_tf - 1.0) * span_sz * 0.55
                            } else {
                                0.0
                            };
                            let span_char_space = char_space + span_tracking;

                            let span_num_chars = span.text.chars().count();
                            let span_w = estimate_str_width_category(&span.text, span_sz, category) * span_wf
                                + span_char_space * (span_num_chars as f64);

                            let span_px = line_px + cos * cum_x;
                            let span_py = line_py + sin * cum_x;

                            let escaped = encode_pdf_string(&span.text);

                            let mut text_ops = String::new();
                            let mut reset_ops = String::new();
                            if span_char_space.abs() > 0.001 {
                                text_ops.push_str(&format!("{:.2} Tc\n", span_char_space));
                                reset_ops.push_str("0 Tc\n");
                            }
                            if (span_wf - 1.0).abs() > 0.01 {
                                let tz = (span_wf * 100.0).clamp(10.0, 500.0);
                                text_ops.push_str(&format!("{:.1} Tz\n", tz));
                                reset_ops.push_str("100 Tz\n");
                            }

                            let shear = if span.oblique_angle.abs() > 0.01 {
                                span.oblique_angle.to_radians().tan()
                            } else {
                                0.0
                            };
                            let span_c = c_vec + shear * cos;
                            let span_d = d_vec + shear * sin;

                            stream.push_str(&format!(
                                "q\n{:.3} {:.3} {:.3} rg\nBT\n/{} {:.2} Tf\n{}{:.4} {:.4} {:.4} {:.4} {:.2} {:.2} Tm\n({}) Tj\n{}ET\nQ\n",
                                pdf_r, pdf_g, pdf_b,
                                span_font, span_sz,
                                text_ops,
                                cos, sin, span_c, span_d, span_px, span_py,
                                escaped,
                                reset_ops
                            ));

                            if span.is_underline {
                                let stroke_y = -0.15 * span_sz;
                                let x0 = line_px + cos * cum_x + c_vec * stroke_y;
                                let y0 = line_py + sin * cum_x + d_vec * stroke_y;
                                let x1 = line_px + cos * (cum_x + span_w) + c_vec * stroke_y;
                                let y1 = line_py + sin * (cum_x + span_w) + d_vec * stroke_y;
                                let sw = (0.06 * span_sz).max(0.2);
                                stream.push_str(&format!(
                                    "q\n{:.3} {:.3} {:.3} RG\n{:.2} w\n{:.2} {:.2} m\n{:.2} {:.2} l\nS\nQ\n",
                                    pdf_r, pdf_g, pdf_b, sw, x0, y0, x1, y1
                                ));
                            }
                            if span.is_strikethrough {
                                let stroke_y = 0.30 * span_sz;
                                let x0 = line_px + cos * cum_x + c_vec * stroke_y;
                                let y0 = line_py + sin * cum_x + d_vec * stroke_y;
                                let x1 = line_px + cos * (cum_x + span_w) + c_vec * stroke_y;
                                let y1 = line_py + sin * (cum_x + span_w) + d_vec * stroke_y;
                                let sw = (0.06 * span_sz).max(0.2);
                                stream.push_str(&format!(
                                    "q\n{:.3} {:.3} {:.3} RG\n{:.2} w\n{:.2} {:.2} m\n{:.2} {:.2} l\nS\nQ\n",
                                    pdf_r, pdf_g, pdf_b, sw, x0, y0, x1, y1
                                ));
                            }
                            if span.is_overline {
                                let stroke_y = 0.75 * span_sz;
                                let x0 = line_px + cos * cum_x + c_vec * stroke_y;
                                let y0 = line_py + sin * cum_x + d_vec * stroke_y;
                                let x1 = line_px + cos * (cum_x + span_w) + c_vec * stroke_y;
                                let y1 = line_py + sin * (cum_x + span_w) + d_vec * stroke_y;
                                let sw = (0.06 * span_sz).max(0.2);
                                stream.push_str(&format!(
                                    "q\n{:.3} {:.3} {:.3} RG\n{:.2} w\n{:.2} {:.2} m\n{:.2} {:.2} l\nS\nQ\n",
                                    pdf_r, pdf_g, pdf_b, sw, x0, y0, x1, y1
                                ));
                            }

                            cum_x += span_w;
                        }
                    }
                }
            }
            EntityType::Hatch(hatch) => {
                if hatch.paths.is_empty() {
                    return;
                }
                let has_geometry = hatch.paths.iter().any(|p| !p.edges.is_empty());
                if !has_geometry {
                    return;
                }

                let needs_clip = hatch.gradient_color.enabled || (!hatch.is_solid && !hatch.pattern.lines.is_empty());
                if needs_clip {
                    stream.push_str("q\n");
                } else if hatch.is_solid {
                    Self::ensure_fill(stream, state, r, g, b);
                } else {
                    Self::ensure_stroke(stream, state, r, g, b, line_width_pt, None);
                }

                for path in &hatch.paths {
                    if path.edges.is_empty() {
                        continue;
                    }
                    let mut is_first = true;
                    for edge in &path.edges {
                        match edge {
                            BoundaryEdge::Line(line) => {
                                let (p1x, p1y) = transform.apply(line.start.x, line.start.y);
                                let (p2x, p2y) = transform.apply(line.end.x, line.end.y);
                                if is_first {
                                    stream.push_str(&format!("{:.2} {:.2} m\n", p1x, p1y));
                                    is_first = false;
                                } else {
                                    stream.push_str(&format!("{:.2} {:.2} l\n", p1x, p1y));
                                }
                                stream.push_str(&format!("{:.2} {:.2} l\n", p2x, p2y));
                            }
                            BoundaryEdge::CircularArc(arc) => {
                                let (cx, cy) = transform.apply(arc.center.x, arc.center.y);
                                let r_rad = arc.radius * transform.scale();
                                let rot = transform.rotation_radians();
                                let (raw_start, raw_end) = if !arc.counter_clockwise {
                                    (-arc.start_angle, -arc.end_angle)
                                } else {
                                    (arc.start_angle, arc.end_angle)
                                };
                                let start_rad = raw_start + rot;
                                let end_rad = raw_end + rot;
                                let ccw = if transform.is_reflected() { !arc.counter_clockwise } else { arc.counter_clockwise };
                                stream.push_str(&generate_arc_path_with_mode(cx, cy, r_rad, start_rad, end_rad, is_first, ccw));
                                is_first = false;
                            }
                            BoundaryEdge::EllipticArc(el) => {
                                let (cx, cy) = transform.apply(el.center.x, el.center.y);
                                let maj_x = el.major_axis_endpoint.x * transform.a + el.major_axis_endpoint.y * transform.c;
                                let maj_y = el.major_axis_endpoint.x * transform.b + el.major_axis_endpoint.y * transform.d;
                                let (start_param, end_param) = if el.start_angle.abs() > 6.283185307179586 || el.end_angle.abs() > 6.283185307179586 {
                                    (el.start_angle.to_radians(), el.end_angle.to_radians())
                                } else {
                                    (el.start_angle, el.end_angle)
                                };
                                let (raw_start, raw_end) = if !el.counter_clockwise {
                                    (-start_param, -end_param)
                                } else {
                                    (start_param, end_param)
                                };
                                let ccw = if transform.is_reflected() { !el.counter_clockwise } else { el.counter_clockwise };
                                stream.push_str(&generate_ellipse_path_with_mode(
                                    cx,
                                    cy,
                                    maj_x,
                                    maj_y,
                                    el.minor_axis_ratio,
                                    raw_start,
                                    raw_end,
                                    is_first,
                                    ccw,
                                ));
                                is_first = false;
                            }
                            BoundaryEdge::Spline(sp) => {
                                let pts: Vec<Point2D> = if sp.knots.len() >= (sp.degree as usize + 1) && !sp.control_points.is_empty() {
                                    let ctrl_pts: Vec<Point2D> = sp.control_points.iter().map(|cp| {
                                        let (x, y) = transform.apply(cp.x, cp.y);
                                        Point2D::new(x, y)
                                    }).collect();
                                    let weights: Vec<f64> = sp.control_points.iter().map(|cp| cp.z).collect();
                                    let num_samples = (ctrl_pts.len() * 16).clamp(32, 256);
                                    sample_bspline(sp.degree as usize, &sp.knots, &ctrl_pts, Some(&weights), num_samples)
                                } else {
                                    sp.control_points.iter().map(|cp| {
                                        let (x, y) = transform.apply(cp.x, cp.y);
                                        Point2D::new(x, y)
                                    }).collect()
                                };
                                if !pts.is_empty() {
                                    if is_first {
                                        stream.push_str(&format!("{:.2} {:.2} m\n", pts[0].x, pts[0].y));
                                        is_first = false;
                                    } else {
                                        stream.push_str(&format!("{:.2} {:.2} l\n", pts[0].x, pts[0].y));
                                    }
                                    for pt in &pts[1..] {
                                        stream.push_str(&format!("{:.2} {:.2} l\n", pt.x, pt.y));
                                    }
                                }
                            }
                            BoundaryEdge::Polyline(poly) => {
                                if !poly.vertices.is_empty() {
                                    let n = poly.vertices.len();
                                    let mut transformed_pts = Vec::with_capacity(n);
                                    for v in &poly.vertices {
                                        let (x, y) = transform.apply(v.x, v.y);
                                        transformed_pts.push((Point2D::new(x, y), v.z));
                                    }
                                    if is_first {
                                        stream.push_str(&format!("{:.2} {:.2} m\n", transformed_pts[0].0.x, transformed_pts[0].0.y));
                                        is_first = false;
                                    } else {
                                        stream.push_str(&format!("{:.2} {:.2} l\n", transformed_pts[0].0.x, transformed_pts[0].0.y));
                                    }
                                    for i in 0..(n - 1) {
                                        let (p1, bulge) = transformed_pts[i];
                                        let (p2, _) = transformed_pts[i + 1];
                                        if bulge.abs() > 1e-6 {
                                            let eff_bulge = if transform.is_reflected() { -bulge } else { bulge };
                                            stream.push_str(&generate_bulge_segment(p1, p2, eff_bulge));
                                        } else {
                                            stream.push_str(&format!("{:.2} {:.2} l\n", p2.x, p2.y));
                                        }
                                    }
                                    if poly.is_closed && n > 1 {
                                        let (p1, bulge) = transformed_pts[n - 1];
                                        let (p2, _) = transformed_pts[0];
                                        if bulge.abs() > 1e-6 {
                                            let eff_bulge = if transform.is_reflected() { -bulge } else { bulge };
                                            stream.push_str(&generate_bulge_segment(p1, p2, eff_bulge));
                                        } else {
                                            stream.push_str(&format!("{:.2} {:.2} l\n", p2.x, p2.y));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    stream.push_str("h\n");
                }

                if needs_clip {
                    stream.push_str("W* n\n");
                    if hatch.gradient_color.enabled {
                        self.render_hatch_gradient(hatch, transform, stream, state, (r, g, b));
                    } else {
                        Self::render_hatch_pattern_lines(hatch, transform, stream, state, r, g, b, line_width_pt);
                    }
                    stream.push_str("Q\n");
                    state.stroke_rgb = None;
                    state.fill_rgb = None;
                    state.line_width = None;
                    state.dash_pattern = None;
                } else if hatch.is_solid {
                    stream.push_str("f*\n");
                } else {
                    stream.push_str("S\n");
                }
            }
            EntityType::Spline(spline) => {
                let pts: Vec<Point2D> = if spline.knots.len() >= (spline.degree as usize + 1) && spline.control_points.len() >= 2 {
                    let ctrl_pts: Vec<Point2D> = spline.control_points.iter().map(|cp| {
                        let (x, y) = transform.apply(cp.x, cp.y);
                        Point2D::new(x, y)
                    }).collect();
                    let num_samples = (ctrl_pts.len() * 16).clamp(32, 256);
                    let weights_opt = if spline.flags.rational && !spline.weights.is_empty() {
                        Some(spline.weights.as_slice())
                    } else {
                        None
                    };
                    sample_bspline(spline.degree as usize, &spline.knots, &ctrl_pts, weights_opt, num_samples)
                } else {
                    spline.control_points.iter().map(|cp| {
                        let (x, y) = transform.apply(cp.x, cp.y);
                        Point2D::new(x, y)
                    }).collect()
                };
                if pts.len() >= 2 {
                    Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                    stream.push_str(&format!("{:.2} {:.2} m\n", pts[0].x, pts[0].y));
                    for pt in &pts[1..] {
                        stream.push_str(&format!("{:.2} {:.2} l\n", pt.x, pt.y));
                    }
                    if spline.flags.closed {
                        stream.push_str("h ");
                    }
                    stream.push_str("S\n");
                }
            }
            EntityType::Tolerance(tol) => {
                self.render_tolerance(tol, transform, stream, state, r, g, b, line_width_pt);
            }
            EntityType::Leader(ldr) => {
                if ldr.vertices.len() >= 2 {
                    Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                    let (p0x, p0y) = transform.apply(ldr.vertices[0].x, ldr.vertices[0].y);
                    let (p1x, p1y) = transform.apply(ldr.vertices[1].x, ldr.vertices[1].y);

                    let dim_style_arrow = self.doc.dim_styles
                        .iter()
                        .find(|ds| ds.name.eq_ignore_ascii_case(&ldr.dimension_style))
                        .and_then(|ds| if ds.dimasz > 0.001 { Some(ds.dimasz) } else { None });

                    let raw_arrow_sz = if ldr.arrow_size > 0.001 {
                        ldr.arrow_size
                    } else if let Some(asz) = dim_style_arrow {
                        asz
                    } else if self.doc.header.dim_arrow_size > 0.001 {
                        self.doc.header.dim_arrow_size
                    } else if ldr.text_height > 0.001 {
                        ldr.text_height * 0.5
                    } else {
                        2.5
                    };

                    let mut arrow_sz = (raw_arrow_sz * transform.scale()).max(3.0);

                    if ldr.arrow_enabled {
                        let dx = p0x - p1x;
                        let dy = p0y - p1y;
                        let len = dx.hypot(dy);
                        if len > 1e-4 {
                            arrow_sz = arrow_sz.min(len * 0.4);
                            let ux = dx / len;
                            let uy = dy / len;
                            let vx = -uy;
                            let vy = ux;
                            let bx = p0x - arrow_sz * ux;
                            let by = p0y - arrow_sz * uy;
                            let w = (arrow_sz * 0.25).max(0.5);

                            // Fill and stroke arrow head
                            Self::ensure_fill(stream, state, r, g, b);
                            Self::ensure_stroke(stream, state, r, g, b, (line_width_pt * 0.8).min(0.5), None);
                            stream.push_str(&format!(
                                "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l h B\n",
                                p0x, p0y,
                                bx + w * vx, by + w * vy,
                                bx - w * vx, by - w * vy
                            ));
                            Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);

                            // Leader line starts from arrow base
                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l\n", bx, by, p1x, p1y));
                        } else {
                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l\n", p0x, p0y, p1x, p1y));
                        }
                    } else {
                        stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l\n", p0x, p0y, p1x, p1y));
                    }

                    for v in &ldr.vertices[2..] {
                        let (px, py) = transform.apply(v.x, v.y);
                        stream.push_str(&format!("{:.2} {:.2} l\n", px, py));
                    }
                    stream.push_str("S\n");
                }
            }
            EntityType::Insert(insert) => {
                let base_pt = self
                    .doc
                    .block_records
                    .get(&insert.block_name)
                    .map(|b| b.base_point)
                    .unwrap_or(Vector3::ZERO);

                let base_transform = Transform2D::from_scale_and_translation(1.0, -base_pt.x, -base_pt.y);

                let cols = insert.column_count.max(1) as usize;
                let rows = insert.row_count.max(1) as usize;
                let col_spacing = insert.column_spacing;
                let row_spacing = insert.row_spacing;
                let cos_rot = insert.rotation.cos();
                let sin_rot = insert.rotation.sin();

                let block_name = &insert.block_name;
                if visited_blocks.insert(block_name.clone()) {
                    for r in 0..rows {
                        for c in 0..cols {
                            let dx = (c as f64) * col_spacing * cos_rot - (r as f64) * row_spacing * sin_rot;
                            let dy = (c as f64) * col_spacing * sin_rot + (r as f64) * row_spacing * cos_rot;

                            let insert_transform = Transform2D::from_trs(
                                insert.insert_point.x + dx,
                                insert.insert_point.y + dy,
                                insert.rotation,
                                insert.x_scale(),
                                insert.y_scale(),
                            );
                            let combined = transform.combine(&insert_transform.combine(&base_transform));

                            let effective_child_color = match &entity_color {
                                Color::Index(0) | Color::ByBlock => parent_color,
                                Color::ByLayer => layer_color,
                                explicit => Some(explicit),
                            };

                            for block_ent in self.get_sorted_entities_in_block(block_name) {
                                if matches!(block_ent, EntityType::AttributeDefinition(_)) {
                                    continue;
                                }
                                self.render_entity(
                                    block_ent,
                                    &combined,
                                    stream,
                                    state,
                                    visited_blocks,
                                    depth + 1,
                                    Some(layer_name),
                                    effective_child_color,
                                );
                            }
                        }
                    }
                    visited_blocks.remove(block_name);
                }

                let effective_child_color = match &entity_color {
                    Color::Index(0) | Color::ByBlock => parent_color,
                    Color::ByLayer => layer_color,
                    explicit => Some(explicit),
                };
                for attrib in &insert.attributes {
                    let dummy_ent = EntityType::AttributeEntity(attrib.clone());
                    self.render_entity(&dummy_ent, transform, stream, state, visited_blocks, depth, Some(layer_name), effective_child_color);
                }
            }
            EntityType::Dimension(dim) => {
                let block_name = &dim.base().block_name;
                if !block_name.is_empty() && visited_blocks.insert(block_name.clone()) {
                    let meas = dim.base().actual_measurement;
                    let needs_scale = meas > 500.0 && self.doc.entities_in_block(block_name).any(|e| {
                        if let EntityType::MText(mt) = e {
                            mt.height > 0.0 && (meas / mt.height) > 2000.0
                        } else {
                            false
                        }
                    });

                    for block_ent in self.doc.entities_in_block(block_name) {
                        if needs_scale {
                            let annotative_scale = 1000.0;
                            match block_ent {
                                EntityType::MText(mt) => {
                                    let mut scaled_mt = mt.clone();
                                    if scaled_mt.height < 1.0 {
                                        scaled_mt.height *= annotative_scale;
                                    }
                                    let ent = EntityType::MText(scaled_mt);
                                    self.render_entity(&ent, transform, stream, state, visited_blocks, depth + 1, Some(layer_name), Some(&entity_color));
                                }
                                EntityType::Solid(s) => {
                                    let mut scaled_s = s.clone();
                                    let cx = (scaled_s.first_corner.x + scaled_s.second_corner.x) * 0.5;
                                    let cy = (scaled_s.first_corner.y + scaled_s.second_corner.y) * 0.5;
                                    let dx = scaled_s.first_corner.x - scaled_s.third_corner.x;
                                    let dy = scaled_s.first_corner.y - scaled_s.third_corner.y;
                                    if dx.hypot(dy) < 2.0 {
                                        scaled_s.first_corner.x = cx + (scaled_s.first_corner.x - cx) * annotative_scale;
                                        scaled_s.first_corner.y = cy + (scaled_s.first_corner.y - cy) * annotative_scale;
                                        scaled_s.second_corner.x = cx + (scaled_s.second_corner.x - cx) * annotative_scale;
                                        scaled_s.second_corner.y = cy + (scaled_s.second_corner.y - cy) * annotative_scale;
                                        scaled_s.third_corner.x = cx + (scaled_s.third_corner.x - cx) * annotative_scale;
                                        scaled_s.third_corner.y = cy + (scaled_s.third_corner.y - cy) * annotative_scale;
                                        scaled_s.fourth_corner.x = cx + (scaled_s.fourth_corner.x - cx) * annotative_scale;
                                        scaled_s.fourth_corner.y = cy + (scaled_s.fourth_corner.y - cy) * annotative_scale;
                                    }
                                    let ent = EntityType::Solid(scaled_s);
                                    self.render_entity(&ent, transform, stream, state, visited_blocks, depth + 1, Some(layer_name), Some(&entity_color));
                                }
                                EntityType::Line(l) => {
                                    let mut scaled_l = l.clone();
                                    let dx = scaled_l.end.x - scaled_l.start.x;
                                    let dy = scaled_l.end.y - scaled_l.start.y;
                                    let len = dx.hypot(dy);
                                    if len > 0.0 && len < 2.0 {
                                        let cx = (scaled_l.start.x + scaled_l.end.x) * 0.5;
                                        let cy = (scaled_l.start.y + scaled_l.end.y) * 0.5;
                                        scaled_l.start.x = cx + (scaled_l.start.x - cx) * annotative_scale;
                                        scaled_l.start.y = cy + (scaled_l.start.y - cy) * annotative_scale;
                                        scaled_l.end.x = cx + (scaled_l.end.x - cx) * annotative_scale;
                                        scaled_l.end.y = cy + (scaled_l.end.y - cy) * annotative_scale;
                                    }
                                    let ent = EntityType::Line(scaled_l);
                                    self.render_entity(&ent, transform, stream, state, visited_blocks, depth + 1, Some(layer_name), Some(&entity_color));
                                }
                                _ => {
                                    self.render_entity(block_ent, transform, stream, state, visited_blocks, depth + 1, Some(layer_name), Some(&entity_color));
                                }
                            }
                        } else {
                            self.render_entity(
                                block_ent,
                                transform,
                                stream,
                                state,
                                visited_blocks,
                                depth + 1,
                                Some(layer_name),
                                Some(&entity_color),
                            );
                        }
                    }
                    visited_blocks.remove(block_name);
                }
            }
            EntityType::PolyfaceMesh(p) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                for face in &p.faces {
                    let indices = [face.index1, face.index2, face.index3, face.index4];
                    let count = if face.index4 != 0 { 4 } else { 3 };
                    for i in 0..count {
                        let idx_signed = indices[i];
                        if idx_signed > 0 {
                            let next_i = (i + 1) % count;
                            let next_idx = indices[next_i].abs() as usize;
                            let curr_idx = idx_signed.abs() as usize;
                            if curr_idx > 0 && curr_idx <= p.vertices.len() && next_idx > 0 && next_idx <= p.vertices.len() {
                                let p1 = &p.vertices[curr_idx - 1];
                                let p2 = &p.vertices[next_idx - 1];
                                let (x1, y1) = transform.apply(p1.location.x, p1.location.y);
                                let (x2, y2) = transform.apply(p2.location.x, p2.location.y);
                                stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", x1, y1, x2, y2));
                            }
                        }
                    }
                }
            }
            EntityType::Mesh(m) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                for face in &m.faces {
                    let n = face.vertices.len();
                    if n < 2 { continue; }
                    for i in 0..n {
                        let idx1 = face.vertices[i] as usize;
                        let idx2 = face.vertices[(i + 1) % n] as usize;
                        if idx1 < m.vertices.len() && idx2 < m.vertices.len() {
                            let p1 = &m.vertices[idx1];
                            let p2 = &m.vertices[idx2];
                            let (x1, y1) = transform.apply(p1.x, p1.y);
                            let (x2, y2) = transform.apply(p2.x, p2.y);
                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", x1, y1, x2, y2));
                        }
                    }
                }
            }
            EntityType::MultiLeader(ml) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                for root in &ml.context.leader_roots {
                    let (cx, cy) = transform.apply(root.connection_point.x, root.connection_point.y);
                    if root.landing_distance > 1e-3 {
                        let dir_len = (root.direction.x * root.direction.x + root.direction.y * root.direction.y).sqrt();
                        let dir = if dir_len > 1e-4 {
                            (root.direction.x / dir_len, root.direction.y / dir_len)
                        } else {
                            (1.0, 0.0)
                        };
                        let land_x = root.connection_point.x + dir.0 * root.landing_distance;
                        let land_y = root.connection_point.y + dir.1 * root.landing_distance;
                        let (lx, ly) = transform.apply(land_x, land_y);
                        stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", cx, cy, lx, ly));
                    }
                    for line in &root.lines {
                        if line.points.is_empty() {
                            continue;
                        }
                        // Build chain of points connecting from connection_point to arrow tip (line.points[0]):
                        // In AutoCAD MLEADER, line.points[0] is the arrow tip pointing to the object.
                        // Intermediate vertices lead from connection_point to the tip.
                        let mut chain: Vec<Vector3> = Vec::new();
                        chain.push(root.connection_point);
                        for pt in line.points.iter().rev() {
                            if let Some(prev) = chain.last() {
                                let dist = ((pt.x - prev.x).powi(2) + (pt.y - prev.y).powi(2)).sqrt();
                                if dist > 1e-3 {
                                    chain.push(*pt);
                                }
                            } else {
                                chain.push(*pt);
                            }
                        }

                        if chain.len() < 2 {
                            continue;
                        }

                        let raw_arrow_sz = if line.arrowhead_size > 0.01 {
                            line.arrowhead_size
                        } else if ml.context.arrowhead_size > 0.01 {
                            ml.context.arrowhead_size
                        } else if ml.arrowhead_size > 0.01 {
                            if ml.arrowhead_size < 1.0 && ml.context.text_height > 10.0 {
                                ml.context.text_height
                            } else {
                                ml.arrowhead_size
                            }
                        } else if ml.context.text_height > 0.01 {
                            ml.context.text_height
                        } else {
                            2.5
                        };
                        let arrow_sz = (raw_arrow_sz * transform.scale()).max(4.0);

                        let pts_pdf: Vec<(f64, f64)> = chain.iter().map(|p| transform.apply(p.x, p.y)).collect();
                        let n_pts = pts_pdf.len();
                        let tip_pt = pts_pdf[n_pts - 1];
                        let prev_pt = pts_pdf[n_pts - 2];

                        let dx = tip_pt.0 - prev_pt.0;
                        let dy = tip_pt.1 - prev_pt.1;
                        let len = dx.hypot(dy);

                        // Draw intermediate segments
                        for i in 0..n_pts - 2 {
                            let p_a = pts_pdf[i];
                            let p_b = pts_pdf[i + 1];
                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", p_a.0, p_a.1, p_b.0, p_b.1));
                        }

                        // Last segment and arrowhead
                        if len > 1e-4 && arrow_sz > 0.0 {
                            let ux = dx / len;
                            let uy = dy / len;
                            let vx = -uy;
                            let vy = ux;
                            let bx = tip_pt.0 - arrow_sz * ux;
                            let by = tip_pt.1 - arrow_sz * uy;
                            let w = (arrow_sz * 0.25).max(1.0);

                            // Segment up to arrow base
                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", prev_pt.0, prev_pt.1, bx, by));

                            // Filled arrowhead triangle at target tip
                            Self::ensure_fill(stream, state, r, g, b);
                            Self::ensure_stroke(stream, state, r, g, b, (line_width_pt * 0.8).min(0.5), None);
                            stream.push_str(&format!(
                                "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l h B\n",
                                tip_pt.0, tip_pt.1,
                                bx + w * vx, by + w * vy,
                                bx - w * vx, by - w * vy
                            ));
                            Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                        } else {
                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", prev_pt.0, prev_pt.1, tip_pt.0, tip_pt.1));
                        }
                    }
                }
                if ml.context.has_text_contents && !ml.context.text_string.is_empty() {
                    let mut dummy_mtext = acadrust::entities::MText::default();
                    dummy_mtext.value = ml.context.text_string.clone();
                    let loc = ml.context.text_location;
                    let rot = ml.context.text_rotation;
                    let width = ml.context.text_width;

                    use acadrust::entities::multileader::TextAttachmentPointType;
                    match ml.context.text_attachment_point {
                        TextAttachmentPointType::Right => {
                            dummy_mtext.attachment_point = acadrust::entities::mtext::AttachmentPoint::TopRight;
                        }
                        TextAttachmentPointType::Center => {
                            dummy_mtext.attachment_point = acadrust::entities::mtext::AttachmentPoint::TopCenter;
                        }
                        _ => {
                            if ml.context.text_direction.x < -0.1 {
                                dummy_mtext.attachment_point = acadrust::entities::mtext::AttachmentPoint::TopRight;
                            } else {
                                dummy_mtext.attachment_point = acadrust::entities::mtext::AttachmentPoint::TopLeft;
                            }
                        }
                    }

                    let norm_rot = if (rot - std::f64::consts::PI).abs() < 1e-2 || (rot + std::f64::consts::PI).abs() < 1e-2 {
                        0.0
                    } else {
                        rot
                    };
                    dummy_mtext.rotation = norm_rot;
                    dummy_mtext.insertion_point = loc;
                    dummy_mtext.height = ml.context.text_height;
                    if width > 0.0 {
                        dummy_mtext.rectangle_width = width;
                    }
                    dummy_mtext.common = ml.common.clone();
                    let ent = EntityType::MText(dummy_mtext);
                    self.render_entity(&ent, transform, stream, state, visited_blocks, depth, None, None);
                }
                if ml.context.has_block_contents || ml.content_type == acadrust::entities::multileader::LeaderContentType::Block {
                    let block_handle = ml.context.block_content_handle.or(ml.block_content_handle);
                    let block_name = block_handle.and_then(|h| {
                        self.doc.block_records.iter().find(|br| br.handle == h).map(|br| br.name.clone())
                    });
                    if let Some(bname) = block_name {
                        let loc = ml.context.block_content_location;
                        let rot = ml.context.block_rotation;
                        let scale = ml.context.block_content_scale;
                        let base_pt = self.doc.block_records.get(&bname).map(|b| b.base_point).unwrap_or(Vector3::ZERO);
                        let base_transform = Transform2D::from_scale_and_translation(1.0, -base_pt.x, -base_pt.y);
                        let insert_transform = Transform2D::from_trs(loc.x, loc.y, rot, scale.x, scale.y);
                        let combined = transform.combine(&insert_transform.combine(&base_transform));

                        let effective_child_color = match &entity_color {
                            Color::Index(0) | Color::ByBlock => parent_color,
                            Color::ByLayer => layer_color,
                            explicit => Some(explicit),
                        };

                        if visited_blocks.insert(bname.clone()) {
                            for block_ent in self.doc.entities_in_block(&bname) {
                                if matches!(block_ent, EntityType::AttributeDefinition(_)) {
                                    continue;
                                }
                                self.render_entity(block_ent, &combined, stream, state, visited_blocks, depth + 1, Some(layer_name), effective_child_color);
                            }
                            visited_blocks.remove(&bname);
                        }
                    }
                }
            }
            EntityType::XLine(xline) => {
                let box_min_x = if self.bbox.is_valid() { self.bbox.min_x } else { xline.base_point.x - 1000.0 };
                let box_max_x = if self.bbox.is_valid() { self.bbox.max_x } else { xline.base_point.x + 1000.0 };
                let box_min_y = if self.bbox.is_valid() { self.bbox.min_y } else { xline.base_point.y - 1000.0 };
                let box_max_y = if self.bbox.is_valid() { self.bbox.max_y } else { xline.base_point.y + 1000.0 };

                let p0 = xline.base_point;
                let dir = xline.direction;
                if let Some((t1, t2)) = clip_line_to_box(p0.x, p0.y, dir.x, dir.y, box_min_x, box_max_x, box_min_y, box_max_y, false) {
                    Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                    let (x1, y1) = transform.apply(p0.x + t1 * dir.x, p0.y + t1 * dir.y);
                    let (x2, y2) = transform.apply(p0.x + t2 * dir.x, p0.y + t2 * dir.y);
                    stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", x1, y1, x2, y2));
                }
            }
            EntityType::Ray(ray) => {
                let box_min_x = if self.bbox.is_valid() { self.bbox.min_x } else { ray.base_point.x - 1000.0 };
                let box_max_x = if self.bbox.is_valid() { self.bbox.max_x } else { ray.base_point.x + 1000.0 };
                let box_min_y = if self.bbox.is_valid() { self.bbox.min_y } else { ray.base_point.y - 1000.0 };
                let box_max_y = if self.bbox.is_valid() { self.bbox.max_y } else { ray.base_point.y + 1000.0 };

                let p0 = ray.base_point;
                let dir = ray.direction;
                if let Some((t1, t2)) = clip_line_to_box(p0.x, p0.y, dir.x, dir.y, box_min_x, box_max_x, box_min_y, box_max_y, true) {
                    Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                    let (x1, y1) = transform.apply(p0.x + t1 * dir.x, p0.y + t1 * dir.y);
                    let (x2, y2) = transform.apply(p0.x + t2 * dir.x, p0.y + t2 * dir.y);
                    stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", x1, y1, x2, y2));
                }
            }
            EntityType::Wipeout(wo) => {
                let pts = wo.world_boundary_vertices();
                if !pts.is_empty() {
                    Self::ensure_fill(stream, state, 1.0, 1.0, 1.0);
                    let (tx0, ty0) = transform.apply(pts[0].x, pts[0].y);
                    stream.push_str(&format!("{:.2} {:.2} m\n", tx0, ty0));

                    for pt in &pts[1..] {
                        let (tx, ty) = transform.apply(pt.x, pt.y);
                        stream.push_str(&format!("{:.2} {:.2} l\n", tx, ty));
                    }
                    stream.push_str("h f\n");
                }
            }
            EntityType::Solid3D(solid) => {
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt, dash_slice);
                for wire in &solid.wires {
                    if wire.points.len() >= 2 {
                        let (p0x, p0y) = transform.apply(wire.points[0].x, wire.points[0].y);
                        stream.push_str(&format!("{:.2} {:.2} m\n", p0x, p0y));
                        for pt in &wire.points[1..] {
                            let (px, py) = transform.apply(pt.x, pt.y);
                            stream.push_str(&format!("{:.2} {:.2} l\n", px, py));
                        }
                        stream.push_str("S\n");
                    }
                }
            }
            EntityType::Ole2Frame(ole) => {
                self.render_ole2frame(ole, transform, stream, state);
            }
            EntityType::RasterImage(img) => {
                let p1x = img.insertion_point.x + img.u_vector.x * img.size.x;
                let p1y = img.insertion_point.y + img.u_vector.y * img.size.x;
                let p2x = p1x + img.v_vector.x * img.size.y;
                let p2y = p1y + img.v_vector.y * img.size.y;
                let p3x = img.insertion_point.x + img.v_vector.x * img.size.y;
                let p3y = img.insertion_point.y + img.v_vector.y * img.size.y;

                let (tx0, ty0) = transform.apply(img.insertion_point.x, img.insertion_point.y);
                let (tx1, ty1) = transform.apply(p1x, p1y);
                let (tx2, ty2) = transform.apply(p2x, p2y);
                let (tx3, ty3) = transform.apply(p3x, p3y);

                // Draw bounding frame
                Self::ensure_stroke(stream, state, r, g, b, line_width_pt.min(0.5), None);
                stream.push_str(&format!(
                    "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h S\n",
                    tx0, ty0, tx1, ty1, tx2, ty2, tx3, ty3
                ));

                // If image file path is missing or file doesn't exist, draw red filename label
                let file_exists = !img.file_path.is_empty() && std::path::Path::new(&img.file_path).exists();
                if !file_exists {
                    let filename = img.file_path
                        .rsplit(|c| c == '/' || c == '\\')
                        .next()
                        .unwrap_or(&img.file_path);
                    if !filename.is_empty() {
                        let center_x = (tx0 + tx2) / 2.0;
                        let center_y = (ty0 + ty2) / 2.0;
                        let font_sz = ((ty2 - ty0).abs() * 0.08).clamp(4.0, 14.0);
                        let w = estimate_str_width(filename, font_sz);
                        Self::ensure_fill(stream, state, 1.0, 0.0, 0.0);
                        let enc = encode_pdf_string(filename);
                        let bold_font_name = self.ensure_font(StandardFont::HelveticaBold);
                        stream.push_str(&format!(
                            "BT\n/{} {:.2} Tf\n1 0 0 1 {:.2} {:.2} Tm\n({}) Tj\nET\n",
                            bold_font_name, font_sz, center_x - w / 2.0, center_y - font_sz * 0.35, enc
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    fn render_ole2frame(
        &mut self,
        ole: &acadrust::entities::Ole2Frame,
        transform: &Transform2D,
        stream: &mut String,
        state: &mut GraphicsState,
    ) {
        let pres = match ole.presentation() {
            Some(p) => p,
            None => return,
        };

        let x_min = ole.upper_left_corner.x.min(ole.lower_right_corner.x);
        let x_max = ole.upper_left_corner.x.max(ole.lower_right_corner.x);
        let y_min = ole.upper_left_corner.y.min(ole.lower_right_corner.y);
        let y_max = ole.upper_left_corner.y.max(ole.lower_right_corner.y);

        let cad_w = (x_max - x_min).max(1e-4);
        let cad_h = (y_max - y_min).max(1e-4);

        match pres {
            OlePresentation::Emf(data) => {
                if data.len() < 88 {
                    return;
                }
                let emf_left = i32::from_le_bytes(data[8..12].try_into().unwrap()) as f64;
                let emf_top = i32::from_le_bytes(data[12..16].try_into().unwrap()) as f64;
                let emf_right = i32::from_le_bytes(data[16..20].try_into().unwrap()) as f64;
                let emf_bottom = i32::from_le_bytes(data[20..24].try_into().unwrap()) as f64;

                let emf_w = (emf_right - emf_left).abs().max(1.0);
                let emf_h = (emf_bottom - emf_top).abs().max(1.0);

                let map_pt = |ex: f64, ey: f64| -> (f64, f64) {
                    let u = (ex - emf_left) / emf_w;
                    let v = (ey - emf_top) / emf_h;
                    let cx = x_min + u * cad_w;
                    let cy = y_max - v * cad_h;
                    transform.apply(cx, cy)
                };

                // OLE tables render with an opaque white background rectangle covering sheet gridlines underneath
                let (p_bl_x, p_bl_y) = transform.apply(x_min, y_min);
                let (p_tr_x, p_tr_y) = transform.apply(x_max, y_max);
                let bg_min_x = p_bl_x.min(p_tr_x);
                let bg_max_x = p_bl_x.max(p_tr_x);
                let bg_min_y = p_bl_y.min(p_tr_y);
                let bg_max_y = p_bl_y.max(p_tr_y);
                let bg_w = bg_max_x - bg_min_x;
                let bg_h = bg_max_y - bg_min_y;
                if bg_w > 0.1 && bg_h > 0.1 {
                    stream.push_str(&format!("q\n1 1 1 rg\n{:.2} {:.2} {:.2} {:.2} re f\nQ\n", bg_min_x, bg_min_y, bg_w, bg_h));
                }

                let mut pos = 0;
                let mut cur_pt = (0.0, 0.0);
                let mut stroke_rgb = (0.2, 0.2, 0.2);
                let mut text_rgb = (0.0, 0.0, 0.0);
                let fill_rgb = (0.9, 0.9, 0.9);
                let mut cur_brush_rgb: Option<(f32, f32, f32)> = Some((0.9, 0.9, 0.9));
                let mut pens: HashMap<u32, (f32, f32, f32)> = HashMap::new();
                let mut brushes: HashMap<u32, Option<(f32, f32, f32)>> = HashMap::new();
                #[derive(Clone, Copy)]
                struct EmfFont {
                    height: f64,
                    is_bold: bool,
                }
                let mut fonts: HashMap<u32, EmfFont> = HashMap::new();
                let mut cur_font: Option<EmfFont> = None;

                while pos + 8 <= data.len() {
                    let i_type = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
                    let n_size = u32::from_le_bytes(data[pos+4..pos+8].try_into().unwrap()) as usize;
                    if n_size < 8 || pos + n_size > data.len() {
                        break;
                    }
                    let rec = &data[pos..pos+n_size];

                    match i_type {
                        24 => {
                            // EMR_SETTEXTCOLOR
                            if rec.len() >= 12 {
                                let c = u32::from_le_bytes(rec[8..12].try_into().unwrap());
                                let r = (c & 0xFF) as f32 / 255.0;
                                let g = ((c >> 8) & 0xFF) as f32 / 255.0;
                                let b = ((c >> 16) & 0xFF) as f32 / 255.0;
                                text_rgb = (r, g, b);
                            }
                        }
                        38 => {
                            // EMR_CREATEPEN
                            if rec.len() >= 24 {
                                let ih_pen = u32::from_le_bytes(rec[8..12].try_into().unwrap());
                                let c = u32::from_le_bytes(rec[20..24].try_into().unwrap());
                                let r = (c & 0xFF) as f32 / 255.0;
                                let g = ((c >> 8) & 0xFF) as f32 / 255.0;
                                let b = ((c >> 16) & 0xFF) as f32 / 255.0;
                                pens.insert(ih_pen, (r, g, b));
                            }
                        }
                        39 => {
                            // EMR_CREATEBRUSHINDIRECT
                            if rec.len() >= 20 {
                                let ih_brush = u32::from_le_bytes(rec[8..12].try_into().unwrap());
                                let lb_style = u32::from_le_bytes(rec[12..16].try_into().unwrap());
                                if lb_style == 1 {
                                    // BS_NULL (hollow brush)
                                    brushes.insert(ih_brush, None);
                                } else {
                                    let c = u32::from_le_bytes(rec[16..20].try_into().unwrap());
                                    let r = (c & 0xFF) as f32 / 255.0;
                                    let g = ((c >> 8) & 0xFF) as f32 / 255.0;
                                    let b = ((c >> 16) & 0xFF) as f32 / 255.0;
                                    brushes.insert(ih_brush, Some((r, g, b)));
                                }
                            }
                        }
                        37 => {
                            // EMR_SELECTOBJECT
                            if rec.len() >= 12 {
                                let ih_obj = u32::from_le_bytes(rec[8..12].try_into().unwrap());
                                if let Some(&c) = pens.get(&ih_obj) {
                                    stroke_rgb = c;
                                }
                                if let Some(&c_opt) = brushes.get(&ih_obj) {
                                    cur_brush_rgb = c_opt;
                                }
                                if let Some(&f) = fonts.get(&ih_obj) {
                                    cur_font = Some(f);
                                }
                            }
                        }
                        40 => {
                            // EMR_DELETEOBJECT
                            if rec.len() >= 12 {
                                let ih_obj = u32::from_le_bytes(rec[8..12].try_into().unwrap());
                                pens.remove(&ih_obj);
                                brushes.remove(&ih_obj);
                                fonts.remove(&ih_obj);
                            }
                        }
                        82 => {
                            // EMR_EXTCREATEFONTINDIRECTW
                            if rec.len() >= 32 {
                                let ih_font = u32::from_le_bytes(rec[8..12].try_into().unwrap());
                                let lf_height = (i32::from_le_bytes(rec[12..16].try_into().unwrap()).abs() as f64).max(1.0);
                                let lf_weight = i32::from_le_bytes(rec[28..32].try_into().unwrap());
                                let is_bold = lf_weight >= 700;
                                fonts.insert(ih_font, EmfFont { height: lf_height, is_bold });
                            }
                        }
                        27 => {
                            if rec.len() >= 16 {
                                let x = i32::from_le_bytes(rec[8..12].try_into().unwrap()) as f64;
                                let y = i32::from_le_bytes(rec[12..16].try_into().unwrap()) as f64;
                                cur_pt = (x, y);
                            }
                        }
                        54 => {
                            if rec.len() >= 16 {
                                let x = i32::from_le_bytes(rec[8..12].try_into().unwrap()) as f64;
                                let y = i32::from_le_bytes(rec[12..16].try_into().unwrap()) as f64;
                                let (p1x, p1y) = map_pt(cur_pt.0, cur_pt.1);
                                let (p2x, p2y) = map_pt(x, y);
                                Self::ensure_stroke(stream, state, stroke_rgb.0, stroke_rgb.1, stroke_rgb.2, 0.0, None);
                                stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", p1x, p1y, p2x, p2y));
                                cur_pt = (x, y);
                            }
                        }
                        43 => {
                            if rec.len() >= 24 {
                                let l = i32::from_le_bytes(rec[8..12].try_into().unwrap()) as f64;
                                let t = i32::from_le_bytes(rec[12..16].try_into().unwrap()) as f64;
                                let r = i32::from_le_bytes(rec[16..20].try_into().unwrap()) as f64;
                                let b = i32::from_le_bytes(rec[20..24].try_into().unwrap()) as f64;
                                let (p1x, p1y) = map_pt(l, t);
                                let (p2x, p2y) = map_pt(r, b);
                                let min_x = p1x.min(p2x);
                                let max_x = p1x.max(p2x);
                                let min_y = p1y.min(p2y);
                                let max_y = p1y.max(p2y);
                                let rw = max_x - min_x;
                                let rh = max_y - min_y;
                                if rw > 0.1 && rh > 0.1 {
                                    Self::ensure_stroke(stream, state, stroke_rgb.0, stroke_rgb.1, stroke_rgb.2, 0.0, None);
                                    stream.push_str(&format!("{:.2} {:.2} {:.2} {:.2} re S\n", min_x, min_y, rw, rh));
                                }
                            }
                        }
                        75 | 76 => {
                            // EMR_STRETCHBLT (75) and EMR_BITBLT (76):
                            // Used in EMF table grids for cell borders, dividers, and background fills.
                            if rec.len() >= 40 {
                                let cb_bits = if rec.len() >= 92 {
                                    u32::from_le_bytes(rec[88..92].try_into().unwrap()) as usize
                                } else {
                                    0
                                };

                                // Only fill solid brush if there are no embedded raster bits
                                if cb_bits == 0 {
                                    if let Some(brush_rgb) = cur_brush_rgb {
                                        let x_dest = i32::from_le_bytes(rec[24..28].try_into().unwrap()) as f64;
                                        let y_dest = i32::from_le_bytes(rec[28..32].try_into().unwrap()) as f64;
                                        let cx_dest = i32::from_le_bytes(rec[32..36].try_into().unwrap()) as f64;
                                        let cy_dest = i32::from_le_bytes(rec[36..40].try_into().unwrap()) as f64;

                                        let ex1 = x_dest.min(x_dest + cx_dest);
                                        let ex2 = x_dest.max(x_dest + cx_dest);
                                        let ey1 = y_dest.min(y_dest + cy_dest);
                                        let ey2 = y_dest.max(y_dest + cy_dest);

                                        let (p1x, p1y) = map_pt(ex1, ey1);
                                        let (p2x, p2y) = map_pt(ex2, ey2);

                                        let min_x = p1x.min(p2x);
                                        let max_x = p1x.max(p2x);
                                        let min_y = p1y.min(p2y);
                                        let max_y = p1y.max(p2y);

                                        if cx_dest.abs() <= 1.0 {
                                            // 1-pixel vertical grid divider line: stroke as 0 w hairline
                                            let mid_x = (p1x + p2x) * 0.5;
                                            Self::ensure_stroke(stream, state, brush_rgb.0, brush_rgb.1, brush_rgb.2, 0.0, None);
                                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", mid_x, min_y, mid_x, max_y));
                                        } else if cy_dest.abs() <= 1.0 {
                                            // 1-pixel horizontal grid divider line: stroke as 0 w hairline
                                            let mid_y = (p1y + p2y) * 0.5;
                                            Self::ensure_stroke(stream, state, brush_rgb.0, brush_rgb.1, brush_rgb.2, 0.0, None);
                                            stream.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", min_x, mid_y, max_x, mid_y));
                                        } else {
                                            let rw = max_x - min_x;
                                            let rh = max_y - min_y;
                                            if rw > 0.001 && rh > 0.001 {
                                                Self::ensure_fill(stream, state, brush_rgb.0, brush_rgb.1, brush_rgb.2);
                                                stream.push_str(&format!("{:.2} {:.2} {:.2} {:.2} re f\n", min_x, min_y, rw, rh));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        86 => {
                            if rec.len() >= 28 {
                                let cpts = u32::from_le_bytes(rec[24..28].try_into().unwrap()) as usize;
                                if cpts >= 3 && 28 + cpts * 4 <= rec.len() {
                                    Self::ensure_fill(stream, state, fill_rgb.0, fill_rgb.1, fill_rgb.2);
                                    let mut pt_offset = 28;
                                    let x0 = i16::from_le_bytes(rec[pt_offset..pt_offset+2].try_into().unwrap()) as f64;
                                    let y0 = i16::from_le_bytes(rec[pt_offset+2..pt_offset+4].try_into().unwrap()) as f64;
                                    let (p0x, p0y) = map_pt(x0, y0);
                                    stream.push_str(&format!("{:.2} {:.2} m\n", p0x, p0y));
                                    pt_offset += 4;
                                    for _ in 1..cpts {
                                        let x = i16::from_le_bytes(rec[pt_offset..pt_offset+2].try_into().unwrap()) as f64;
                                        let y = i16::from_le_bytes(rec[pt_offset+2..pt_offset+4].try_into().unwrap()) as f64;
                                        let (px, py) = map_pt(x, y);
                                        stream.push_str(&format!("{:.2} {:.2} l\n", px, py));
                                        pt_offset += 4;
                                    }
                                    stream.push_str("h f\n");
                                }
                            }
                        }
                        87 => {
                            if rec.len() >= 28 {
                                let cpts = u32::from_le_bytes(rec[24..28].try_into().unwrap()) as usize;
                                if cpts >= 2 && 28 + cpts * 4 <= rec.len() {
                                    Self::ensure_stroke(stream, state, stroke_rgb.0, stroke_rgb.1, stroke_rgb.2, 0.0, None);
                                    let mut pt_offset = 28;
                                    let x0 = i16::from_le_bytes(rec[pt_offset..pt_offset+2].try_into().unwrap()) as f64;
                                    let y0 = i16::from_le_bytes(rec[pt_offset+2..pt_offset+4].try_into().unwrap()) as f64;
                                    let (p0x, p0y) = map_pt(x0, y0);
                                    stream.push_str(&format!("{:.2} {:.2} m\n", p0x, p0y));
                                    pt_offset += 4;
                                    for _ in 1..cpts {
                                        let x = i16::from_le_bytes(rec[pt_offset..pt_offset+2].try_into().unwrap()) as f64;
                                        let y = i16::from_le_bytes(rec[pt_offset+2..pt_offset+4].try_into().unwrap()) as f64;
                                        let (px, py) = map_pt(x, y);
                                        stream.push_str(&format!("{:.2} {:.2} l\n", px, py));
                                        pt_offset += 4;
                                    }
                                    stream.push_str("S\n");
                                }
                            }
                        }
                        84 => {
                            if rec.len() >= 52 {
                                let ref_x = i32::from_le_bytes(rec[36..40].try_into().unwrap()) as f64;
                                let ref_y = i32::from_le_bytes(rec[40..44].try_into().unwrap()) as f64;
                                let n_chars = u32::from_le_bytes(rec[44..48].try_into().unwrap()) as usize;
                                let off_str = u32::from_le_bytes(rec[48..52].try_into().unwrap()) as usize;

                                if n_chars > 0 && off_str + n_chars * 2 <= rec.len() {
                                    let u16_slice: Vec<u16> = rec[off_str..off_str + n_chars * 2]
                                        .chunks_exact(2)
                                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                                        .collect();
                                    let text = String::from_utf16_lossy(&u16_slice);
                                    let trimmed = text.trim();
                                    if !trimmed.is_empty() {
                                        let (tx, ty) = map_pt(ref_x, ref_y);
                                        let (font_h, is_bold) = cur_font
                                            .map(|f| (f.height, f.is_bold))
                                            .unwrap_or_else(|| {
                                                let rcl_top = i32::from_le_bytes(rec[12..16].try_into().unwrap()) as f64;
                                                let rcl_bottom = i32::from_le_bytes(rec[20..24].try_into().unwrap()) as f64;
                                                let rh = (rcl_bottom - rcl_top).abs();
                                                (if rh > 0.0 { rh } else { 16.0 }, false)
                                            });
                                        let font_sz = ((font_h / emf_h) * cad_h * transform.scale()).max(0.05);
                                        let baseline_y = ty - font_sz * 0.75;

                                        Self::ensure_fill(stream, state, text_rgb.0, text_rgb.1, text_rgb.2);
                                        let enc = encode_pdf_string(trimmed);
                                        let font = if is_bold { StandardFont::HelveticaBold } else { StandardFont::Helvetica };
                                        let primary_font_name = self.ensure_font(font);
                                        stream.push_str(&format!(
                                            "BT\n/{} {:.2} Tf\n1 0 0 1 {:.2} {:.2} Tm\n({}) Tj\nET\n",
                                            primary_font_name, font_sz, tx, baseline_y, enc
                                        ));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    pos += n_size;
                }
            }
            OlePresentation::Raster(_) => {}
            _ => {}
        }
    }
}

fn clip_line_to_box(
    px: f64,
    py: f64,
    dx: f64,
    dy: f64,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    is_ray: bool,
) -> Option<(f64, f64)> {
    let mut t_min = if is_ray { 0.0 } else { f64::NEG_INFINITY };
    let mut t_max = f64::INFINITY;

    if dx.abs() < 1e-9 {
        if px < min_x || px > max_x {
            return None;
        }
    } else {
        let t1 = (min_x - px) / dx;
        let t2 = (max_x - px) / dx;
        t_min = t_min.max(t1.min(t2));
        t_max = t_max.min(t1.max(t2));
    }

    if dy.abs() < 1e-9 {
        if py < min_y || py > max_y {
            return None;
        }
    } else {
        let t1 = (min_y - py) / dy;
        let t2 = (max_y - py) / dy;
        t_min = t_min.max(t1.min(t2));
        t_max = t_max.min(t1.max(t2));
    }

    if t_min <= t_max {
        Some((t_min, t_max))
    } else {
        None
    }
}

pub(crate) fn char_to_winansi(ch: char) -> Option<u8> {
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
        0x2205 | 0x2300 => Some(0xD8), // ∅ / ⌀ (Diameter mapped to Ø in WinAnsi)
        0x00B1 => Some(0xB1),        // ±
        0x00B0 => Some(0xB0),        // °
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

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::Line;
    use acadrust::types::Handle;

    #[test]
    fn empty_and_non_renderable_paper_space_layouts_produce_only_model_space() {
        let mut doc = CadDocument::new();

        // Add visible geometry in *Model_Space
        let line = Line::from_coords(0.0, 0.0, 0.0, 100.0, 50.0, 0.0);
        doc.add_entity(EntityType::Line(line)).unwrap();

        // Add non-renderable entities (invisible and Defpoints) into *Paper_Space
        let mut inv_line = Line::from_coords(0.0, 0.0, 0.0, 10.0, 10.0, 0.0);
        inv_line.common.invisible = true;
        inv_line.common.entity_mode = Some(1);
        doc.add_entity(EntityType::Line(inv_line)).unwrap();

        let mut defpoints_line = Line::from_coords(0.0, 0.0, 0.0, 10.0, 10.0, 0.0);
        defpoints_line.common.layer = "Defpoints".to_string();
        defpoints_line.common.entity_mode = Some(1);
        doc.add_entity(EntityType::Line(defpoints_line)).unwrap();

        // Add an unlinked empty extra Layout
        let mut extra_layout = acadrust::objects::Layout::new("EmptyLayout");
        extra_layout.handle = Handle::new(0x9999);
        extra_layout.tab_order = 2;
        doc.objects.insert(extra_layout.handle, ObjectType::Layout(extra_layout));

        let pdf_bytes = crate::pdf::convert_cad_to_pdf(&doc, ConversionOptions::default())
            .expect("synthetic document conversion should succeed");
        let pdf_doc = hipdf::lopdf::Document::load_mem(&pdf_bytes).expect("valid PDF");
        assert_eq!(pdf_doc.get_pages().len(), 1, "expected only Model space page");
    }
}
