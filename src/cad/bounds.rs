use acadrust::document::CadDocument;
use acadrust::entities::{BoundaryEdge, EntityType};
use acadrust::types::Matrix3;
use acadrust::Vector3;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Default for BoundingBox {
    fn default() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }
}

impl BoundingBox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_valid(&self) -> bool {
        self.min_x.is_finite()
            && self.min_y.is_finite()
            && self.max_x.is_finite()
            && self.max_y.is_finite()
            && self.max_x >= self.min_x
            && self.max_y >= self.min_y
    }

    pub fn width(&self) -> f64 {
        if self.is_valid() {
            (self.max_x - self.min_x).max(0.0)
        } else {
            0.0
        }
    }

    pub fn height(&self) -> f64 {
        if self.is_valid() {
            (self.max_y - self.min_y).max(0.0)
        } else {
            0.0
        }
    }

    pub fn update_point(&mut self, x: f64, y: f64) {
        if x.is_finite() && y.is_finite() {
            self.min_x = self.min_x.min(x);
            self.min_y = self.min_y.min(y);
            self.max_x = self.max_x.max(x);
            self.max_y = self.max_y.max(y);
        }
    }

    pub fn update_bbox(&mut self, other: &BoundingBox) {
        if other.is_valid() {
            self.min_x = self.min_x.min(other.min_x);
            self.min_y = self.min_y.min(other.min_y);
            self.max_x = self.max_x.max(other.max_x);
            self.max_y = self.max_y.max(other.max_y);
        }
    }

    pub fn intersects(&self, other: &BoundingBox) -> bool {
        self.is_valid()
            && other.is_valid()
            && self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Transform2D {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::identity()
    }
}

impl Transform2D {
    pub fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    pub fn from_scale_and_translation(scale: f64, tx: f64, ty: f64) -> Self {
        Self {
            a: scale,
            b: 0.0,
            c: 0.0,
            d: scale,
            tx,
            ty,
        }
    }

    pub fn from_trs(tx: f64, ty: f64, rotation_rad: f64, sx: f64, sy: f64) -> Self {
        let cos = rotation_rad.cos();
        let sin = rotation_rad.sin();
        Self {
            a: sx * cos,
            b: sx * sin,
            c: -sy * sin,
            d: sy * cos,
            tx,
            ty,
        }
    }

    pub fn scale(&self) -> f64 {
        self.a.hypot(self.b)
    }

    pub fn rotation_radians(&self) -> f64 {
        self.b.atan2(self.a)
    }

    pub fn is_reflected(&self) -> bool {
        (self.a * self.d - self.b * self.c) < 0.0
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }

    pub fn apply_delta(&self, dx: f64, dy: f64) -> (f64, f64) {
        (
            self.a * dx + self.c * dy,
            self.b * dx + self.d * dy,
        )
    }

    pub fn combine(&self, other: &Transform2D) -> Transform2D {
        Transform2D {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            tx: self.a * other.tx + self.c * other.ty + self.tx,
            ty: self.b * other.tx + self.d * other.ty + self.ty,
        }
    }
}

/// Compute bounding box for a CAD document with outlier detection for model space.
pub fn calculate_document_bounds(
    doc: &CadDocument,
    model_space_only: bool,
) -> BoundingBox {
    let entities: Vec<&EntityType> = if model_space_only {
        doc.model_space_entities().collect()
    } else {
        doc.entities().collect()
    };

    let mut entity_boxes = Vec::new();
    for entity in entities {
        let dyn_ent = entity.as_entity();
        if dyn_ent.is_invisible() {
            continue;
        }

        let layer_name = dyn_ent.layer();
        if layer_name.eq_ignore_ascii_case("defpoints") {
            continue;
        }
        if let Some(layer) = doc.layers.iter().find(|l| l.name.eq_ignore_ascii_case(layer_name)) {
            if !layer.is_plottable || layer.is_frozen() || layer.is_off() {
                continue;
            }
        }

        let mut ent_box = BoundingBox::new();
        let mut visited_blocks = std::collections::HashSet::new();
        accumulate_entity_bounds(doc, entity, &Transform2D::identity(), &mut ent_box, &mut visited_blocks, 0, None);
        if ent_box.is_valid() {
            entity_boxes.push(ent_box);
        }
    }

    if entity_boxes.is_empty() {
        return BoundingBox {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 100.0,
            max_y: 100.0,
        };
    }

    if entity_boxes.len() <= 8 || !model_space_only {
        let mut bbox = BoundingBox::new();
        for b in entity_boxes {
            bbox.update_bbox(&b);
        }
        return bbox;
    }

    // Statistical IQR outlier detection for Model Space
    let mut xs: Vec<f64> = entity_boxes.iter().map(|b| (b.min_x + b.max_x) * 0.5).collect();
    let mut ys: Vec<f64> = entity_boxes.iter().map(|b| (b.min_y + b.max_y) * 0.5).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let n = xs.len();
    let q1_idx = n / 4;
    let q3_idx = (3 * n) / 4;

    let q1_x = xs[q1_idx];
    let q3_x = xs[q3_idx];
    let iqr_x = (q3_x - q1_x).max(1.0);

    let q1_y = ys[q1_idx];
    let q3_y = ys[q3_idx];
    let iqr_y = (q3_y - q1_y).max(1.0);

    let scale = iqr_x.max(iqr_y);
    let eff_iqr_x = iqr_x.max(scale * 0.25);
    let eff_iqr_y = iqr_y.max(scale * 0.25);

    let mut lower_x = q1_x - 15.0 * eff_iqr_x;
    let mut upper_x = q3_x + 15.0 * eff_iqr_x;
    let mut lower_y = q1_y - 15.0 * eff_iqr_y;
    let mut upper_y = q3_y + 15.0 * eff_iqr_y;

    // Respect valid AutoCAD header extents only if they are not extreme outliers
    let h_min = doc.header.model_space_extents_min;
    let h_max = doc.header.model_space_extents_max;
    if h_max.x > h_min.x && h_max.y > h_min.y {
        let h_span_x = h_max.x - h_min.x;
        let h_span_y = h_max.y - h_min.y;
        if h_span_x <= 25.0 * eff_iqr_x && h_span_y <= 25.0 * eff_iqr_y
            && h_min.x >= q1_x - 20.0 * eff_iqr_x && h_max.x <= q3_x + 20.0 * eff_iqr_x
            && h_min.y >= q1_y - 20.0 * eff_iqr_y && h_max.y <= q3_y + 20.0 * eff_iqr_y
        {
            lower_x = lower_x.min(h_min.x - 10.0);
            upper_x = upper_x.max(h_max.x + 10.0);
            lower_y = lower_y.min(h_min.y - 10.0);
            upper_y = upper_y.max(h_max.y + 10.0);
        }
    }

    let mut inlier_bbox = BoundingBox::new();
    let mut inlier_count = 0;
    for b in &entity_boxes {
        // Reject entities that are individually hundreds of times larger than the IQR
        if b.width() > 50.0 * eff_iqr_x || b.height() > 50.0 * eff_iqr_y {
            continue;
        }
        let cx = (b.min_x + b.max_x) * 0.5;
        let cy = (b.min_y + b.max_y) * 0.5;
        if cx >= lower_x && cx <= upper_x && cy >= lower_y && cy <= upper_y {
            inlier_bbox.update_bbox(b);
            inlier_count += 1;
        }
    }

    if inlier_count > 0 && inlier_bbox.is_valid() {
        inlier_bbox
    } else {
        let mut bbox = BoundingBox::new();
        for b in entity_boxes {
            bbox.update_bbox(&b);
        }
        bbox
    }
}

/// Compute bounding box for all plottable entities in a block.
pub fn calculate_block_bounds(doc: &CadDocument, block_name: &str) -> BoundingBox {
    let mut bbox = BoundingBox::new();
    let mut visited = std::collections::HashSet::new();
    visited.insert(block_name.to_string());
    for entity in doc.entities_in_block(block_name) {
        let dyn_ent = entity.as_entity();
        if dyn_ent.is_invisible() {
            continue;
        }
        let layer_name = dyn_ent.layer();
        if layer_name.eq_ignore_ascii_case("defpoints") {
            continue;
        }
        if let Some(layer) = doc.layers.iter().find(|l| l.name.eq_ignore_ascii_case(layer_name)) {
            if !layer.is_plottable || layer.is_frozen() || layer.is_off() {
                continue;
            }
        }
        if matches!(entity, EntityType::AttributeDefinition(_)) {
            continue;
        }
        accumulate_entity_bounds(doc, entity, &Transform2D::identity(), &mut bbox, &mut visited, 0, None);
    }
    bbox
}

pub fn accumulate_entity_bounds(
    doc: &CadDocument,
    entity: &EntityType,
    transform: &Transform2D,
    bbox: &mut BoundingBox,
    visited_blocks: &mut std::collections::HashSet<String>,
    depth: usize,
    parent_layer: Option<&str>,
) {
    if depth > 32 {
        return;
    }

    let dyn_ent = entity.as_entity();
    if dyn_ent.is_invisible() {
        return;
    }

    let raw_layer = dyn_ent.layer();
    let layer_name = if raw_layer == "0" {
        parent_layer.unwrap_or(raw_layer)
    } else {
        raw_layer
    };

    if layer_name.eq_ignore_ascii_case("defpoints") {
        return;
    }
    if let Some(layer) = doc.layers.iter().find(|l| l.name.eq_ignore_ascii_case(layer_name)) {
        if !layer.is_plottable || layer.is_frozen() || layer.is_off() {
            return;
        }
    }

    match entity {
        EntityType::Line(line) => {
            let (x1, y1) = transform.apply(line.start.x, line.start.y);
            let (x2, y2) = transform.apply(line.end.x, line.end.y);
            bbox.update_point(x1, y1);
            bbox.update_point(x2, y2);
        }
        EntityType::Circle(circle) => {
            let center = circle.center_wcs();
            let (cx, cy) = transform.apply(center.x, center.y);
            let r = circle.radius * transform.scale();
            bbox.update_point(cx - r, cy - r);
            bbox.update_point(cx + r, cy + r);
        }
        EntityType::Arc(arc) => {
            let center = arc.center_wcs();
            let (cx, cy) = transform.apply(center.x, center.y);
            let r = arc.radius * transform.scale();
            bbox.update_point(cx - r, cy - r);
            bbox.update_point(cx + r, cy + r);
        }
        EntityType::Ellipse(ellipse) => {
            let (cx, cy) = transform.apply(ellipse.center.x, ellipse.center.y);
            let major_len = ellipse.major_axis.x.hypot(ellipse.major_axis.y);
            let r = major_len * transform.scale();
            bbox.update_point(cx - r, cy - r);
            bbox.update_point(cx + r, cy + r);
        }
        EntityType::LwPolyline(lwpoly) => {
            for v in &lwpoly.vertices {
                let (x, y) = transform.apply(v.location.x, v.location.y);
                bbox.update_point(x, y);
            }
            for i in 0..lwpoly.vertices.len() {
                let v1 = &lwpoly.vertices[i];
                if v1.bulge.abs() > 1e-6 {
                    let next_idx = if i + 1 < lwpoly.vertices.len() {
                        i + 1
                    } else if lwpoly.is_closed {
                        0
                    } else {
                        continue;
                    };
                    let v2 = &lwpoly.vertices[next_idx];
                    let mx = (v1.location.x + v2.location.x) / 2.0;
                    let my = (v1.location.y + v2.location.y) / 2.0;
                    let dx = v2.location.x - v1.location.x;
                    let dy = v2.location.y - v1.location.y;
                    let crest_x = mx - dy * v1.bulge * 0.5;
                    let crest_y = my + dx * v1.bulge * 0.5;
                    let (tx, ty) = transform.apply(crest_x, crest_y);
                    bbox.update_point(tx, ty);
                }
            }
        }
        EntityType::Polyline(poly) => {
            for v in &poly.vertices {
                let (x, y) = transform.apply(v.location.x, v.location.y);
                bbox.update_point(x, y);
            }
        }
        EntityType::Polyline2D(poly) => {
            for v in &poly.vertices {
                let (x, y) = transform.apply(v.location.x, v.location.y);
                bbox.update_point(x, y);
            }
        }
        EntityType::Polyline3D(poly) => {
            for v in &poly.vertices {
                let (x, y) = transform.apply(v.position.x, v.position.y);
                bbox.update_point(x, y);
            }
        }
        EntityType::Point(pt) => {
            let (x, y) = transform.apply(pt.location.x, pt.location.y);
            let pd_size = doc.header.point_display_size;
            let mode = doc.header.point_display_mode;
            if mode != 1 {
                let r = if pd_size > 0.0 {
                    pd_size * 0.5 * transform.scale()
                } else {
                    1.0
                };
                bbox.update_point(x - r, y - r);
                bbox.update_point(x + r, y + r);
            } else {
                bbox.update_point(x, y);
            }
        }
        EntityType::Solid(solid) => {
            let (x1, y1) = transform.apply(solid.first_corner.x, solid.first_corner.y);
            let (x2, y2) = transform.apply(solid.second_corner.x, solid.second_corner.y);
            let (x3, y3) = transform.apply(solid.third_corner.x, solid.third_corner.y);
            let (x4, y4) = transform.apply(solid.fourth_corner.x, solid.fourth_corner.y);
            bbox.update_point(x1, y1);
            bbox.update_point(x2, y2);
            bbox.update_point(x3, y3);
            bbox.update_point(x4, y4);
        }
        EntityType::Face3D(face) => {
            let (x1, y1) = transform.apply(face.first_corner.x, face.first_corner.y);
            let (x2, y2) = transform.apply(face.second_corner.x, face.second_corner.y);
            let (x3, y3) = transform.apply(face.third_corner.x, face.third_corner.y);
            let (x4, y4) = transform.apply(face.fourth_corner.x, face.fourth_corner.y);
            bbox.update_point(x1, y1);
            bbox.update_point(x2, y2);
            bbox.update_point(x3, y3);
            bbox.update_point(x4, y4);
        }
        EntityType::Text(txt) => {
            let m = Matrix3::arbitrary_axis(txt.normal);
            let p0 = m.transform_point(txt.insertion_point);
            let u = m.transform_point(Vector3::new(txt.rotation.cos(), txt.rotation.sin(), 0.0));
            let v = m.transform_point(Vector3::new(-txt.rotation.sin(), txt.rotation.cos(), 0.0));
            let flip_x = if (txt.generation_flags & 2) != 0 { -1.0 } else { 1.0 };
            let flip_y = if (txt.generation_flags & 4) != 0 { -1.0 } else { 1.0 };
            let ux = u.x * flip_x;
            let uy = u.y * flip_x;
            let vx = v.x * flip_y;
            let vy = v.y * flip_y;
            let w = (txt.value.len() as f64) * txt.height * 0.7 * txt.width_factor.max(0.1);
            let h = txt.height.max(0.1);
            for &(dx, dy) in &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)] {
                let wx = p0.x + dx * ux + dy * vx;
                let wy = p0.y + dx * uy + dy * vy;
                let (tx, ty) = transform.apply(wx, wy);
                bbox.update_point(tx, ty);
            }
            if let Some(ref ap) = txt.alignment_point {
                let ap_w = m.transform_point(*ap);
                let (ax, ay) = transform.apply(ap_w.x, ap_w.y);
                bbox.update_point(ax, ay);
            }
        }
        EntityType::AttributeDefinition(attdef) => {
            if depth > 0 || attdef.flags.invisible || attdef.default_value.trim().is_empty() {
                return;
            }
            let m = Matrix3::arbitrary_axis(attdef.normal);
            let p0 = m.transform_point(attdef.insertion_point);
            let u = m.transform_point(Vector3::new(attdef.rotation.cos(), attdef.rotation.sin(), 0.0));
            let v = m.transform_point(Vector3::new(-attdef.rotation.sin(), attdef.rotation.cos(), 0.0));
            let flip_x = if (attdef.text_generation_flags & 2) != 0 { -1.0 } else { 1.0 };
            let flip_y = if (attdef.text_generation_flags & 4) != 0 { -1.0 } else { 1.0 };
            let ux = u.x * flip_x;
            let uy = u.y * flip_x;
            let vx = v.x * flip_y;
            let vy = v.y * flip_y;
            let val = attdef.default_value.trim();
            let w = (val.len() as f64) * attdef.height * 0.7 * attdef.width_factor.max(0.1);
            let h = attdef.height.max(0.1);
            for &(dx, dy) in &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)] {
                let wx = p0.x + dx * ux + dy * vx;
                let wy = p0.y + dx * uy + dy * vy;
                let (tx, ty) = transform.apply(wx, wy);
                bbox.update_point(tx, ty);
            }
            let ap_w = m.transform_point(attdef.alignment_point);
            let (ax, ay) = transform.apply(ap_w.x, ap_w.y);
            bbox.update_point(ax, ay);
        }
        EntityType::AttributeEntity(attrib) => {
            if attrib.flags.invisible || attrib.value.trim().is_empty() {
                return;
            }
            let m = Matrix3::arbitrary_axis(attrib.normal);
            let p0 = m.transform_point(attrib.insertion_point);
            let u = m.transform_point(Vector3::new(attrib.rotation.cos(), attrib.rotation.sin(), 0.0));
            let v = m.transform_point(Vector3::new(-attrib.rotation.sin(), attrib.rotation.cos(), 0.0));
            let flip_x = if (attrib.text_generation_flags & 2) != 0 { -1.0 } else { 1.0 };
            let flip_y = if (attrib.text_generation_flags & 4) != 0 { -1.0 } else { 1.0 };
            let ux = u.x * flip_x;
            let uy = u.y * flip_x;
            let vx = v.x * flip_y;
            let vy = v.y * flip_y;
            let val = attrib.value.trim();
            let w = (val.len() as f64) * attrib.height * 0.7 * attrib.width_factor.max(0.1);
            let h = attrib.height.max(0.1);
            for &(dx, dy) in &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)] {
                let wx = p0.x + dx * ux + dy * vx;
                let wy = p0.y + dx * uy + dy * vy;
                let (tx, ty) = transform.apply(wx, wy);
                bbox.update_point(tx, ty);
            }
            let ap_w = m.transform_point(attrib.alignment_point);
            let (ax, ay) = transform.apply(ap_w.x, ap_w.y);
            bbox.update_point(ax, ay);
        }
        EntityType::MText(mtext) => {
            let p0 = mtext.insertion_point;
            let sign_z = if mtext.normal.z.is_sign_negative() { -1.0 } else { 1.0 };
            let u = Vector3::new(mtext.rotation.cos(), mtext.rotation.sin(), 0.0);
            let v = Vector3::new(-mtext.rotation.sin() * sign_z, mtext.rotation.cos() * sign_z, 0.0);
            let w = if mtext.rectangle_width > 0.0 {
                mtext.rectangle_width
            } else {
                (mtext.value.len() as f64) * mtext.height * 0.7
            };
            let h = mtext.height.max(0.1);
            for &(dx, dy) in &[(0.0, 0.0), (w, 0.0), (w, -h), (0.0, -h)] {
                let wx = p0.x + dx * u.x + dy * v.x;
                let wy = p0.y + dx * u.y + dy * v.y;
                let (tx, ty) = transform.apply(wx, wy);
                bbox.update_point(tx, ty);
            }
        }
        EntityType::Hatch(hatch) => {
            for path in &hatch.paths {
                for edge in &path.edges {
                    match edge {
                        BoundaryEdge::Line(line) => {
                            let (x1, y1) = transform.apply(line.start.x, line.start.y);
                            let (x2, y2) = transform.apply(line.end.x, line.end.y);
                            bbox.update_point(x1, y1);
                            bbox.update_point(x2, y2);
                        }
                        BoundaryEdge::CircularArc(arc) => {
                            let (cx, cy) = transform.apply(arc.center.x, arc.center.y);
                            let r = arc.radius * transform.scale();
                            bbox.update_point(cx - r, cy - r);
                            bbox.update_point(cx + r, cy + r);
                        }
                        BoundaryEdge::EllipticArc(el) => {
                            let (cx, cy) = transform.apply(el.center.x, el.center.y);
                            let r = el.major_axis_endpoint.x.hypot(el.major_axis_endpoint.y) * transform.scale();
                            bbox.update_point(cx - r, cy - r);
                            bbox.update_point(cx + r, cy + r);
                        }
                        BoundaryEdge::Spline(sp) => {
                            for pt in &sp.control_points {
                                let (x, y) = transform.apply(pt.x, pt.y);
                                bbox.update_point(x, y);
                            }
                        }
                        BoundaryEdge::Polyline(poly) => {
                            for v in &poly.vertices {
                                let (x, y) = transform.apply(v.x, v.y);
                                bbox.update_point(x, y);
                            }
                        }
                    }
                }
            }
        }
        EntityType::Spline(spline) => {
            for pt in &spline.control_points {
                let (x, y) = transform.apply(pt.x, pt.y);
                bbox.update_point(x, y);
            }
            for pt in &spline.fit_points {
                let (x, y) = transform.apply(pt.x, pt.y);
                bbox.update_point(x, y);
            }
        }
        EntityType::Leader(ldr) => {
            for v in &ldr.vertices {
                let (x, y) = transform.apply(v.x, v.y);
                bbox.update_point(x, y);
            }
        }
        EntityType::Insert(insert) => {
            let base_pt = doc
                .block_records
                .get(&insert.block_name)
                .map(|b| b.base_point)
                .unwrap_or(acadrust::Vector3::ZERO);

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

                        for block_ent in doc.entities_in_block(block_name) {
                            if matches!(block_ent, EntityType::AttributeDefinition(_)) {
                                continue;
                            }
                            accumulate_entity_bounds(doc, block_ent, &combined, bbox, visited_blocks, depth + 1, Some(layer_name));
                        }
                    }
                }
                visited_blocks.remove(block_name);
            }
            for attrib in &insert.attributes {
                let dummy_ent = EntityType::AttributeEntity(attrib.clone());
                accumulate_entity_bounds(doc, &dummy_ent, transform, bbox, visited_blocks, depth, None);
            }
        }
        EntityType::Dimension(dim) => {
            let block_name = &dim.base().block_name;
            if !block_name.is_empty() && visited_blocks.insert(block_name.clone()) {
                for block_ent in doc.entities_in_block(block_name) {
                    accumulate_entity_bounds(doc, block_ent, transform, bbox, visited_blocks, depth + 1, None);
                }
                visited_blocks.remove(block_name);
            }
        }
        EntityType::PolyfaceMesh(p) => {
            for v in &p.vertices {
                let (x, y) = transform.apply(v.location.x, v.location.y);
                bbox.update_point(x, y);
            }
        }
        EntityType::Mesh(m) => {
            for v in &m.vertices {
                let (x, y) = transform.apply(v.x, v.y);
                bbox.update_point(x, y);
            }
        }
        EntityType::MultiLeader(ml) => {
            for root in &ml.context.leader_roots {
                let (cx, cy) = transform.apply(root.connection_point.x, root.connection_point.y);
                bbox.update_point(cx, cy);
                for line in &root.lines {
                    for pt in &line.points {
                        let (px, py) = transform.apply(pt.x, pt.y);
                        bbox.update_point(px, py);
                    }
                }
            }
            if ml.context.has_text_contents && !ml.context.text_string.is_empty() {
                let (tx, ty) = transform.apply(ml.context.text_location.x, ml.context.text_location.y);
                let h = (ml.context.text_height * transform.scale()).max(0.1);
                let w = (ml.context.text_string.len() as f64) * h * 0.7;
                bbox.update_point(tx, ty);
                bbox.update_point(tx + w, ty + h);
            }
            if ml.context.has_block_contents || ml.content_type == acadrust::entities::multileader::LeaderContentType::Block {
                let block_handle = ml.context.block_content_handle.or(ml.block_content_handle);
                let block_name = block_handle.and_then(|h| {
                    doc.block_records.iter().find(|br| br.handle == h).map(|br| br.name.clone())
                });
                if let Some(bname) = block_name {
                    let loc = ml.context.block_content_location;
                    let rot = ml.context.block_rotation;
                    let scale = ml.context.block_content_scale;
                    let base_pt = doc.block_records.get(&bname).map(|b| b.base_point).unwrap_or(acadrust::Vector3::ZERO);
                    let base_transform = Transform2D::from_scale_and_translation(1.0, -base_pt.x, -base_pt.y);
                    let insert_transform = Transform2D::from_trs(loc.x, loc.y, rot, scale.x, scale.y);
                    let combined = transform.combine(&insert_transform.combine(&base_transform));

                    if visited_blocks.insert(bname.clone()) {
                        for block_ent in doc.entities_in_block(&bname) {
                            if matches!(block_ent, EntityType::AttributeDefinition(_)) {
                                continue;
                            }
                            accumulate_entity_bounds(doc, block_ent, &combined, bbox, visited_blocks, depth + 1, Some(layer_name));
                        }
                        visited_blocks.remove(&bname);
                    }
                }
            }
        }
        EntityType::Ray(_) | EntityType::XLine(_) => {
            // Infinite construction lines do not expand drawing extents.
        }
        EntityType::Wipeout(wo) => {
            for pt in wo.world_boundary_vertices() {
                let (tx, ty) = transform.apply(pt.x, pt.y);
                bbox.update_point(tx, ty);
            }
        }
        EntityType::Solid3D(solid) => {
            for wire in &solid.wires {
                for pt in &wire.points {
                    let (tx, ty) = transform.apply(pt.x, pt.y);
                    bbox.update_point(tx, ty);
                }
            }
        }
        EntityType::Ole2Frame(ole) => {
            let (tx1, ty1) = transform.apply(ole.upper_left_corner.x, ole.upper_left_corner.y);
            let (tx2, ty2) = transform.apply(ole.lower_right_corner.x, ole.lower_right_corner.y);
            bbox.update_point(tx1, ty1);
            bbox.update_point(tx2, ty2);
        }
        EntityType::RasterImage(img) => {
            let (tx1, ty1) = transform.apply(img.insertion_point.x, img.insertion_point.y);
            let p2x = img.insertion_point.x + img.u_vector.x * img.size.x + img.v_vector.x * img.size.y;
            let p2y = img.insertion_point.y + img.u_vector.y * img.size.x + img.v_vector.y * img.size.y;
            let (tx2, ty2) = transform.apply(p2x, p2y);
            bbox.update_point(tx1, ty1);
            bbox.update_point(tx2, ty2);
        }
        EntityType::Tolerance(tol) => {
            let text_h = if tol.text_height > 1e-4 { tol.text_height } else { 2.5 };
            let gap = if tol.dimension_gap > 1e-4 { tol.dimension_gap } else { text_h * 0.4 };
            let row_h = text_h + 2.0 * gap;
            let lines: Vec<&str> = tol.text.split(|c| c == '\n' || c == '\r').flat_map(|s| s.split("^J")).collect();
            let n_rows = lines.len().max(1) as f64;
            let mut max_row_w = row_h * 2.0;
            for line in &lines {
                let cells: Vec<&str> = line.split("%%v").collect();
                let row_w: f64 = cells.iter()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| {
                        let clean = s.replace("{\\Fgdt;", "").replace("{\\FGDT;", "").replace("}", "");
                        (clean.len() as f64 * text_h * 0.7 + 2.0 * gap).max(row_h)
                    })
                    .sum();
                max_row_w = max_row_w.max(row_w);
            }
            let total_h = n_rows * row_h;
            let dir_len = tol.direction.x.hypot(tol.direction.y);
            let (ux, uy) = if dir_len > 1e-6 { (tol.direction.x / dir_len, tol.direction.y / dir_len) } else { (1.0, 0.0) };
            let (dx, dy) = (uy, -ux);
            let p0 = (tol.insertion_point.x, tol.insertion_point.y);
            let p1 = (p0.0 + max_row_w * ux, p0.1 + max_row_w * uy);
            let p2 = (p1.0 + total_h * dx, p1.1 + total_h * dy);
            let p3 = (p0.0 + total_h * dx, p0.1 + total_h * dy);
            for pt in [p0, p1, p2, p3] {
                let (tx, ty) = transform.apply(pt.0, pt.1);
                bbox.update_point(tx, ty);
            }
        }
        _ => {}
    }
}

/// Fast conservative bounding box intersection test for viewport culling.
pub fn entity_intersects_bbox(
    doc: &CadDocument,
    entity: &EntityType,
    target_box: &BoundingBox,
    block_bounds_cache: &mut HashMap<String, BoundingBox>,
) -> bool {
    if !target_box.is_valid() {
        return true;
    }

    match entity {
        EntityType::Line(line) => {
            let min_x = line.start.x.min(line.end.x);
            let max_x = line.start.x.max(line.end.x);
            let min_y = line.start.y.min(line.end.y);
            let max_y = line.start.y.max(line.end.y);
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::Circle(c) => {
            let center = c.center_wcs();
            let r = c.radius.abs();
            (center.x - r) <= target_box.max_x
                && (center.x + r) >= target_box.min_x
                && (center.y - r) <= target_box.max_y
                && (center.y + r) >= target_box.min_y
        }
        EntityType::Arc(a) => {
            let center = a.center_wcs();
            let r = a.radius.abs();
            (center.x - r) <= target_box.max_x
                && (center.x + r) >= target_box.min_x
                && (center.y - r) <= target_box.max_y
                && (center.y + r) >= target_box.min_y
        }
        EntityType::Ellipse(e) => {
            let major_len = e.major_axis.x.hypot(e.major_axis.y);
            (e.center.x - major_len) <= target_box.max_x
                && (e.center.x + major_len) >= target_box.min_x
                && (e.center.y - major_len) <= target_box.max_y
                && (e.center.y + major_len) >= target_box.min_y
        }
        EntityType::LwPolyline(lw) => {
            if lw.vertices.is_empty() {
                return false;
            }
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for v in &lw.vertices {
                min_x = min_x.min(v.location.x);
                max_x = max_x.max(v.location.x);
                min_y = min_y.min(v.location.y);
                max_y = max_y.max(v.location.y);
            }
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::Polyline(poly) => {
            if poly.vertices.is_empty() {
                return false;
            }
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for v in &poly.vertices {
                min_x = min_x.min(v.location.x);
                max_x = max_x.max(v.location.x);
                min_y = min_y.min(v.location.y);
                max_y = max_y.max(v.location.y);
            }
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::Polyline2D(poly) => {
            if poly.vertices.is_empty() {
                return false;
            }
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for v in &poly.vertices {
                min_x = min_x.min(v.location.x);
                max_x = max_x.max(v.location.x);
                min_y = min_y.min(v.location.y);
                max_y = max_y.max(v.location.y);
            }
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::Polyline3D(poly) => {
            if poly.vertices.is_empty() {
                return false;
            }
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for v in &poly.vertices {
                min_x = min_x.min(v.position.x);
                max_x = max_x.max(v.position.x);
                min_y = min_y.min(v.position.y);
                max_y = max_y.max(v.position.y);
            }
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::Point(pt) => {
            pt.location.x >= target_box.min_x
                && pt.location.x <= target_box.max_x
                && pt.location.y >= target_box.min_y
                && pt.location.y <= target_box.max_y
        }
        EntityType::Text(txt) => {
            let m = Matrix3::arbitrary_axis(txt.normal);
            let p0 = m.transform_point(txt.insertion_point);
            let u_cos = txt.rotation.cos();
            let u_sin = txt.rotation.sin();
            let w = (txt.value.len() as f64) * txt.height * 0.7 * txt.width_factor.max(0.1);
            let h = txt.height.max(0.1);
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for &(dx, dy) in &[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)] {
                let wx = p0.x + dx * u_cos - dy * u_sin;
                let wy = p0.y + dx * u_sin + dy * u_cos;
                min_x = min_x.min(wx);
                max_x = max_x.max(wx);
                min_y = min_y.min(wy);
                max_y = max_y.max(wy);
            }
            min_x <= target_box.max_x && max_x >= target_box.min_x && min_y <= target_box.max_y && max_y >= target_box.min_y
        }
        EntityType::MText(mtext) => {
            let p0 = mtext.insertion_point;
            let sign_z = if mtext.normal.z.is_sign_negative() { -1.0 } else { 1.0 };
            let u_cos = mtext.rotation.cos();
            let u_sin = mtext.rotation.sin();
            let v_x = -u_sin * sign_z;
            let v_y = u_cos * sign_z;
            let w = if mtext.rectangle_width > 0.0 {
                mtext.rectangle_width
            } else {
                (mtext.value.len() as f64) * mtext.height * 0.7
            };
            let h = if let Some(eh) = mtext.rectangle_height {
                eh.max(mtext.height)
            } else if mtext.extents_height > 0.0 {
                mtext.extents_height
            } else {
                mtext.height.max(0.1) * 3.0
            };
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for &(dx, dy) in &[(0.0, 0.0), (w, 0.0), (w, -h), (0.0, -h)] {
                let wx = p0.x + dx * u_cos + dy * v_x;
                let wy = p0.y + dx * u_sin + dy * v_y;
                min_x = min_x.min(wx);
                max_x = max_x.max(wx);
                min_y = min_y.min(wy);
                max_y = max_y.max(wy);
            }
            min_x <= target_box.max_x && max_x >= target_box.min_x && min_y <= target_box.max_y && max_y >= target_box.min_y
        }
        EntityType::AttributeDefinition(attdef) => {
            if attdef.flags.invisible || attdef.default_value.trim().is_empty() {
                return false;
            }
            let m = Matrix3::arbitrary_axis(attdef.normal);
            let p0 = m.transform_point(attdef.insertion_point);
            let margin = (attdef.height * (attdef.default_value.trim().len() as f64).max(1.0) * 0.8).max(10.0);
            (p0.x - margin) <= target_box.max_x
                && (p0.x + margin) >= target_box.min_x
                && (p0.y - margin) <= target_box.max_y
                && (p0.y + margin) >= target_box.min_y
        }
        EntityType::AttributeEntity(attrib) => {
            if attrib.flags.invisible || attrib.value.trim().is_empty() {
                return false;
            }
            let m = Matrix3::arbitrary_axis(attrib.normal);
            let p0 = m.transform_point(attrib.insertion_point);
            let margin = (attrib.height * (attrib.value.len() as f64).max(1.0) * 0.8).max(10.0);
            (p0.x - margin) <= target_box.max_x
                && (p0.x + margin) >= target_box.min_x
                && (p0.y - margin) <= target_box.max_y
                && (p0.y + margin) >= target_box.min_y
        }
        EntityType::Solid(solid) => {
            let min_x = solid.first_corner.x.min(solid.second_corner.x).min(solid.third_corner.x).min(solid.fourth_corner.x);
            let max_x = solid.first_corner.x.max(solid.second_corner.x).max(solid.third_corner.x).max(solid.fourth_corner.x);
            let min_y = solid.first_corner.y.min(solid.second_corner.y).min(solid.third_corner.y).min(solid.fourth_corner.y);
            let max_y = solid.first_corner.y.max(solid.second_corner.y).max(solid.third_corner.y).max(solid.fourth_corner.y);
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::Face3D(face) => {
            let min_x = face.first_corner.x.min(face.second_corner.x).min(face.third_corner.x).min(face.fourth_corner.x);
            let max_x = face.first_corner.x.max(face.second_corner.x).max(face.third_corner.x).max(face.fourth_corner.x);
            let min_y = face.first_corner.y.min(face.second_corner.y).min(face.third_corner.y).min(face.fourth_corner.y);
            let max_y = face.first_corner.y.max(face.second_corner.y).max(face.third_corner.y).max(face.fourth_corner.y);
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::Insert(insert) => {
            let block_box = block_bounds_cache
                .entry(insert.block_name.clone())
                .or_insert_with(|| calculate_block_bounds(doc, &insert.block_name));

            if !block_box.is_valid() {
                return insert.insert_point.x >= target_box.min_x - 100.0
                    && insert.insert_point.x <= target_box.max_x + 100.0
                    && insert.insert_point.y >= target_box.min_y - 100.0
                    && insert.insert_point.y <= target_box.max_y + 100.0;
            }

            let base_pt = doc
                .block_records
                .get(&insert.block_name)
                .map(|b| b.base_point)
                .unwrap_or(acadrust::Vector3::ZERO);

            let base_transform = Transform2D::from_scale_and_translation(1.0, -base_pt.x, -base_pt.y);
            let insert_transform = Transform2D::from_trs(
                insert.insert_point.x,
                insert.insert_point.y,
                insert.rotation,
                insert.x_scale(),
                insert.y_scale(),
            );
            let combined_insert = insert_transform.combine(&base_transform);
            let (x1, y1) = combined_insert.apply(block_box.min_x, block_box.min_y);
            let (x2, y2) = combined_insert.apply(block_box.max_x, block_box.min_y);
            let (x3, y3) = combined_insert.apply(block_box.max_x, block_box.max_y);
            let (x4, y4) = combined_insert.apply(block_box.min_x, block_box.max_y);
            let min_x = x1.min(x2).min(x3).min(x4);
            let max_x = x1.max(x2).max(x3).max(x4);
            let min_y = y1.min(y2).min(y3).min(y4);
            let max_y = y1.max(y2).max(y3).max(y4);
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::Dimension(dim) => {
            let block_name = &dim.base().block_name;
            if !block_name.is_empty() {
                let dim_box = block_bounds_cache
                    .entry(block_name.clone())
                    .or_insert_with(|| calculate_block_bounds(doc, block_name));
                if dim_box.is_valid() {
                    return dim_box.min_x <= target_box.max_x
                        && dim_box.max_x >= target_box.min_x
                        && dim_box.min_y <= target_box.max_y
                        && dim_box.max_y >= target_box.min_y;
                }
            }
            let def_pt = dim.base().definition_point;
            let text_pt = dim.base().text_middle_point;
            let min_x = def_pt.x.min(text_pt.x) - 50.0;
            let max_x = def_pt.x.max(text_pt.x) + 50.0;
            let min_y = def_pt.y.min(text_pt.y) - 50.0;
            let max_y = def_pt.y.max(text_pt.y) + 50.0;
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::Leader(ldr) => {
            if ldr.vertices.is_empty() {
                return false;
            }
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for v in &ldr.vertices {
                min_x = min_x.min(v.x);
                max_x = max_x.max(v.x);
                min_y = min_y.min(v.y);
                max_y = max_y.max(v.y);
            }
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::Spline(sp) => {
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for pt in &sp.control_points {
                min_x = min_x.min(pt.x);
                max_x = max_x.max(pt.x);
                min_y = min_y.min(pt.y);
                max_y = max_y.max(pt.y);
            }
            for pt in &sp.fit_points {
                min_x = min_x.min(pt.x);
                max_x = max_x.max(pt.x);
                min_y = min_y.min(pt.y);
                max_y = max_y.max(pt.y);
            }
            if min_x.is_infinite() {
                return false;
            }
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::Hatch(hatch) => {
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for path in &hatch.paths {
                for edge in &path.edges {
                    match edge {
                        BoundaryEdge::Line(line) => {
                            min_x = min_x.min(line.start.x).min(line.end.x);
                            max_x = max_x.max(line.start.x).max(line.end.x);
                            min_y = min_y.min(line.start.y).min(line.end.y);
                            max_y = max_y.max(line.start.y).max(line.end.y);
                        }
                        BoundaryEdge::CircularArc(arc) => {
                            let r = arc.radius.abs();
                            min_x = min_x.min(arc.center.x - r);
                            max_x = max_x.max(arc.center.x + r);
                            min_y = min_y.min(arc.center.y - r);
                            max_y = max_y.max(arc.center.y + r);
                        }
                        BoundaryEdge::EllipticArc(el) => {
                            let r = el.major_axis_endpoint.x.hypot(el.major_axis_endpoint.y);
                            min_x = min_x.min(el.center.x - r);
                            max_x = max_x.max(el.center.x + r);
                            min_y = min_y.min(el.center.y - r);
                            max_y = max_y.max(el.center.y + r);
                        }
                        BoundaryEdge::Spline(sp) => {
                            for pt in &sp.control_points {
                                min_x = min_x.min(pt.x);
                                max_x = max_x.max(pt.x);
                                min_y = min_y.min(pt.y);
                                max_y = max_y.max(pt.y);
                            }
                        }
                        BoundaryEdge::Polyline(poly) => {
                            for v in &poly.vertices {
                                min_x = min_x.min(v.x);
                                max_x = max_x.max(v.x);
                                min_y = min_y.min(v.y);
                                max_y = max_y.max(v.y);
                            }
                        }
                    }
                }
            }
            if min_x.is_infinite() {
                return false;
            }
            min_x <= target_box.max_x
                && max_x >= target_box.min_x
                && min_y <= target_box.max_y
                && max_y >= target_box.min_y
        }
        EntityType::MultiLeader(ml) => {
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for root in &ml.context.leader_roots {
                min_x = min_x.min(root.connection_point.x);
                max_x = max_x.max(root.connection_point.x);
                min_y = min_y.min(root.connection_point.y);
                max_y = max_y.max(root.connection_point.y);
                for line in &root.lines {
                    for pt in &line.points {
                        min_x = min_x.min(pt.x);
                        max_x = max_x.max(pt.x);
                        min_y = min_y.min(pt.y);
                        max_y = max_y.max(pt.y);
                    }
                }
            }
            let tp = ml.context.text_location;
            let tw = ml.context.text_width.max(10.0);
            let th = ml.context.text_height.max(10.0);
            min_x = min_x.min(tp.x - tw);
            max_x = max_x.max(tp.x + tw);
            min_y = min_y.min(tp.y - th);
            max_y = max_y.max(tp.y + th);
            if min_x.is_infinite() { return false; }
            min_x <= target_box.max_x && max_x >= target_box.min_x && min_y <= target_box.max_y && max_y >= target_box.min_y
        }
        EntityType::Solid3D(solid) => {
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for wire in &solid.wires {
                for pt in &wire.points {
                    min_x = min_x.min(pt.x);
                    max_x = max_x.max(pt.x);
                    min_y = min_y.min(pt.y);
                    max_y = max_y.max(pt.y);
                }
            }
            if min_x.is_infinite() { return false; }
            min_x <= target_box.max_x && max_x >= target_box.min_x && min_y <= target_box.max_y && max_y >= target_box.min_y
        }
        EntityType::Ole2Frame(ole) => {
            let min_x = ole.upper_left_corner.x.min(ole.lower_right_corner.x);
            let max_x = ole.upper_left_corner.x.max(ole.lower_right_corner.x);
            let min_y = ole.upper_left_corner.y.min(ole.lower_right_corner.y);
            let max_y = ole.upper_left_corner.y.max(ole.lower_right_corner.y);
            min_x <= target_box.max_x && max_x >= target_box.min_x && min_y <= target_box.max_y && max_y >= target_box.min_y
        }
        EntityType::RasterImage(img) => {
            let p1x = img.insertion_point.x;
            let p1y = img.insertion_point.y;
            let p2x = p1x + img.u_vector.x * img.size.x + img.v_vector.x * img.size.y;
            let p2y = p1y + img.u_vector.y * img.size.x + img.v_vector.y * img.size.y;
            let min_x = p1x.min(p2x);
            let max_x = p1x.max(p2x);
            let min_y = p1y.min(p2y);
            let max_y = p1y.max(p2y);
            min_x <= target_box.max_x && max_x >= target_box.min_x && min_y <= target_box.max_y && max_y >= target_box.min_y
        }
        EntityType::Tolerance(tol) => {
            let text_h = if tol.text_height > 1e-4 { tol.text_height } else { 2.5 };
            let gap = if tol.dimension_gap > 1e-4 { tol.dimension_gap } else { text_h * 0.4 };
            let row_h = text_h + 2.0 * gap;
            let lines: Vec<&str> = tol.text.split(|c| c == '\n' || c == '\r').flat_map(|s| s.split("^J")).collect();
            let n_rows = lines.len().max(1) as f64;
            let mut max_row_w = row_h * 2.0;
            for line in &lines {
                let cells: Vec<&str> = line.split("%%v").collect();
                let row_w: f64 = cells.iter()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| {
                        let clean = s.replace("{\\Fgdt;", "").replace("{\\FGDT;", "").replace("}", "");
                        (clean.len() as f64 * text_h * 0.7 + 2.0 * gap).max(row_h)
                    })
                    .sum();
                max_row_w = max_row_w.max(row_w);
            }
            let total_h = n_rows * row_h;
            let dir_len = tol.direction.x.hypot(tol.direction.y);
            let (ux, uy) = if dir_len > 1e-6 { (tol.direction.x / dir_len, tol.direction.y / dir_len) } else { (1.0, 0.0) };
            let (dx, dy) = (uy, -ux);
            let p0 = (tol.insertion_point.x, tol.insertion_point.y);
            let p1 = (p0.0 + max_row_w * ux, p0.1 + max_row_w * uy);
            let p2 = (p1.0 + total_h * dx, p1.1 + total_h * dy);
            let p3 = (p0.0 + total_h * dx, p0.1 + total_h * dy);
            let min_x = p0.0.min(p1.0).min(p2.0).min(p3.0);
            let max_x = p0.0.max(p1.0).max(p2.0).max(p3.0);
            let min_y = p0.1.min(p1.1).min(p2.1).min(p3.1);
            let max_y = p0.1.max(p1.1).max(p2.1).max(p3.1);
            min_x <= target_box.max_x && max_x >= target_box.min_x && min_y <= target_box.max_y && max_y >= target_box.min_y
        }
        _ => false,
    }
}
