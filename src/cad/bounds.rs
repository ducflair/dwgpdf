use acadrust::document::CadDocument;
use acadrust::entities::{BoundaryEdge, EntityType};
use std::collections::HashMap;
use std::f64;

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

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
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
        if let Some(layer) = doc.layers.iter().find(|l| l.name == layer_name) {
            if !layer.is_plottable || layer.is_frozen() || layer.is_off() {
                continue;
            }
        }

        let mut ent_box = BoundingBox::new();
        let mut visited_blocks = std::collections::HashSet::new();
        accumulate_entity_bounds(doc, entity, &Transform2D::identity(), &mut ent_box, &mut visited_blocks, 0);
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

    // Multiplier of 3.0 represents Tukey's outer fence ("extreme outliers")
    let lower_x = q1_x - 3.0 * iqr_x;
    let upper_x = q3_x + 3.0 * iqr_x;
    let lower_y = q1_y - 3.0 * iqr_y;
    let upper_y = q3_y + 3.0 * iqr_y;

    let mut inlier_bbox = BoundingBox::new();
    let mut inlier_count = 0;
    for b in &entity_boxes {
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
        if let Some(layer) = doc.layers.iter().find(|l| l.name == layer_name) {
            if !layer.is_plottable || layer.is_frozen() || layer.is_off() {
                continue;
            }
        }
        accumulate_entity_bounds(doc, entity, &Transform2D::identity(), &mut bbox, &mut visited, 0);
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
    if let Some(layer) = doc.layers.iter().find(|l| l.name == layer_name) {
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
            let (cx, cy) = transform.apply(circle.center.x, circle.center.y);
            let r = circle.radius * transform.scale();
            bbox.update_point(cx - r, cy - r);
            bbox.update_point(cx + r, cy + r);
        }
        EntityType::Arc(arc) => {
            let (cx, cy) = transform.apply(arc.center.x, arc.center.y);
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
            bbox.update_point(x, y);
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
            let (x, y) = transform.apply(txt.insertion_point.x, txt.insertion_point.y);
            let h = (txt.height * transform.scale()).max(0.1);
            let w = (txt.value.len() as f64) * h * 0.7;
            bbox.update_point(x, y);
            bbox.update_point(x + w, y + h);
        }
        EntityType::MText(mtext) => {
            let (x, y) = transform.apply(mtext.insertion_point.x, mtext.insertion_point.y);
            let h = (mtext.height * transform.scale()).max(0.1);
            let w = if mtext.rectangle_width > 0.0 {
                mtext.rectangle_width * transform.scale()
            } else {
                (mtext.value.len() as f64) * h * 0.7
            };
            bbox.update_point(x, y);
            bbox.update_point(x + w, y - h);
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
                for block_ent in doc.entities_in_block(block_name) {
                    accumulate_entity_bounds(doc, block_ent, &combined, bbox, visited_blocks, depth + 1);
                }
                visited_blocks.remove(block_name);
            }
        }
        EntityType::Dimension(dim) => {
            let block_name = &dim.base().block_name;
            if !block_name.is_empty() && visited_blocks.insert(block_name.clone()) {
                for block_ent in doc.entities_in_block(block_name) {
                    accumulate_entity_bounds(doc, block_ent, transform, bbox, visited_blocks, depth + 1);
                }
                visited_blocks.remove(block_name);
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
            let r = c.radius.abs();
            (c.center.x - r) <= target_box.max_x
                && (c.center.x + r) >= target_box.min_x
                && (c.center.y - r) <= target_box.max_y
                && (c.center.y + r) >= target_box.min_y
        }
        EntityType::Arc(a) => {
            let r = a.radius.abs();
            (a.center.x - r) <= target_box.max_x
                && (a.center.x + r) >= target_box.min_x
                && (a.center.y - r) <= target_box.max_y
                && (a.center.y + r) >= target_box.min_y
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
            let margin = (txt.height * 20.0).max(100.0);
            (txt.insertion_point.x - margin) <= target_box.max_x
                && (txt.insertion_point.x + margin) >= target_box.min_x
                && (txt.insertion_point.y - margin) <= target_box.max_y
                && (txt.insertion_point.y + margin) >= target_box.min_y
        }
        EntityType::MText(mtext) => {
            let margin = (mtext.height * 20.0).max(100.0);
            (mtext.insertion_point.x - margin) <= target_box.max_x
                && (mtext.insertion_point.x + margin) >= target_box.min_x
                && (mtext.insertion_point.y - margin) <= target_box.max_y
                && (mtext.insertion_point.y + margin) >= target_box.min_y
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

            let insert_transform = Transform2D::from_trs(
                insert.insert_point.x,
                insert.insert_point.y,
                insert.rotation,
                insert.x_scale(),
                insert.y_scale(),
            );
            let (x1, y1) = insert_transform.apply(block_box.min_x, block_box.min_y);
            let (x2, y2) = insert_transform.apply(block_box.max_x, block_box.min_y);
            let (x3, y3) = insert_transform.apply(block_box.max_x, block_box.max_y);
            let (x4, y4) = insert_transform.apply(block_box.min_x, block_box.max_y);
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
        _ => true,
    }
}
