use acadrust::document::CadDocument;
use acadrust::entities::EntityType;
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

    pub fn from_trs(tx: f64, ty: f64, rotation_deg: f64, sx: f64, sy: f64) -> Self {
        let rad = rotation_deg.to_radians();
        let cos = rad.cos();
        let sin = rad.sin();
        Self {
            a: sx * cos,
            b: sx * sin,
            c: -sy * sin,
            d: sy * cos,
            tx,
            ty,
        }
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

/// Compute bounding box for a CAD document.
pub fn calculate_document_bounds(
    doc: &CadDocument,
    model_space_only: bool,
) -> BoundingBox {
    let mut bbox = BoundingBox::new();

    let entities: Vec<&EntityType> = if model_space_only {
        doc.model_space_entities().collect()
    } else {
        doc.entities().collect()
    };

    let mut visited_blocks = std::collections::HashSet::new();
    for entity in entities {
        accumulate_entity_bounds(doc, entity, &Transform2D::identity(), &mut bbox, &mut visited_blocks, 0);
    }

    if !bbox.is_valid() {
        bbox.min_x = 0.0;
        bbox.min_y = 0.0;
        bbox.max_x = 100.0;
        bbox.max_y = 100.0;
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
    if let Some(layer) = doc.layers.iter().find(|l| l.name == layer_name) {
        if layer.is_frozen() || layer.is_off() {
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
            let r = circle.radius * transform.a.hypot(transform.b);
            bbox.update_point(cx - r, cy - r);
            bbox.update_point(cx + r, cy + r);
        }
        EntityType::Arc(arc) => {
            let (cx, cy) = transform.apply(arc.center.x, arc.center.y);
            let r = arc.radius * transform.a.hypot(transform.b);
            bbox.update_point(cx - r, cy - r);
            bbox.update_point(cx + r, cy + r);
        }
        EntityType::Ellipse(ellipse) => {
            let (cx, cy) = transform.apply(ellipse.center.x, ellipse.center.y);
            let major_len = ellipse.major_axis.x.hypot(ellipse.major_axis.y);
            let r = major_len * transform.a.hypot(transform.b);
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
            let h = txt.height.max(1.0);
            let w = (txt.value.len() as f64) * h * 0.7;
            bbox.update_point(x, y);
            bbox.update_point(x + w, y + h);
        }
        EntityType::MText(mtext) => {
            let (x, y) = transform.apply(mtext.insertion_point.x, mtext.insertion_point.y);
            let h = mtext.height.max(1.0);
            let w = if mtext.rectangle_width > 0.0 {
                mtext.rectangle_width
            } else {
                (mtext.value.len() as f64) * h * 0.7
            };
            bbox.update_point(x, y);
            bbox.update_point(x + w, y - h);
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
