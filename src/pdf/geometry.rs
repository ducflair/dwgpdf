use std::f64::consts::PI;

/// A 2D point for PDF rendering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

impl Point2D {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Generates PDF path operations for a circle using 4 cubic Bézier curves.
pub fn generate_circle_path(cx: f64, cy: f64, radius: f64) -> String {
    let r = radius.abs();
    if r < 1e-6 {
        return format!("{:.4} {:.4} m\n", cx, cy);
    }
    // kappa = 4 * (sqrt(2) - 1) / 3
    let k = 0.5522847498307936 * r;

    format!(
        "{:.4} {:.4} m\n\
         {:.4} {:.4} {:.4} {:.4} {:.4} {:.4} c\n\
         {:.4} {:.4} {:.4} {:.4} {:.4} {:.4} c\n\
         {:.4} {:.4} {:.4} {:.4} {:.4} {:.4} c\n\
         {:.4} {:.4} {:.4} {:.4} {:.4} {:.4} c\n\
         h\n",
        cx + r, cy,
        cx + r, cy + k, cx + k, cy + r, cx, cy + r,
        cx - k, cy + r, cx - r, cy + k, cx - r, cy,
        cx - r, cy - k, cx - k, cy - r, cx, cy - r,
        cx + k, cy - r, cx + r, cy - k, cx + r, cy
    )
}

/// Generates PDF path operations for a circular arc from start_angle to end_angle (in radians).
/// Standard CAD direction is CCW.
pub fn generate_arc_path(cx: f64, cy: f64, radius: f64, start_angle: f64, end_angle: f64) -> String {
    let r = radius.abs();
    if r < 1e-6 {
        return format!("{:.4} {:.4} m\n", cx, cy);
    }

    let mut start = start_angle;
    let mut end = end_angle;

    // Normalize angles
    while start < 0.0 {
        start += 2.0 * PI;
    }
    while end <= start {
        end += 2.0 * PI;
    }

    let total_angle = end - start;
    if total_angle >= 2.0 * PI - 1e-5 {
        return generate_circle_path(cx, cy, r);
    }

    // Number of segments (each <= 90 degrees = PI/2)
    let num_segments = ((total_angle / (PI / 2.0)).ceil() as usize).max(1);
    let step = total_angle / (num_segments as f64);

    let mut out = String::new();
    let p0_x = cx + r * start.cos();
    let p0_y = cy + r * start.sin();
    out.push_str(&format!("{:.4} {:.4} m\n", p0_x, p0_y));

    for i in 0..num_segments {
        let a1 = start + (i as f64) * step;
        let a2 = a1 + step;
        let da = step;

        let k = (4.0 / 3.0) * (da / 4.0).tan();

        let cos1 = a1.cos();
        let sin1 = a1.sin();
        let cos2 = a2.cos();
        let sin2 = a2.sin();

        let cp1_x = cx + r * (cos1 - k * sin1);
        let cp1_y = cy + r * (sin1 + k * cos1);
        let cp2_x = cx + r * (cos2 + k * sin2);
        let cp2_y = cy + r * (sin2 - k * cos2);
        let ep_x = cx + r * cos2;
        let ep_y = cy + r * sin2;

        out.push_str(&format!(
            "{:.4} {:.4} {:.4} {:.4} {:.4} {:.4} c\n",
            cp1_x, cp1_y, cp2_x, cp2_y, ep_x, ep_y
        ));
    }

    out
}

/// Generates PDF path for a bulged segment from p1 to p2.
/// In DXF: bulge = tan(included_angle / 4).
pub fn generate_bulge_segment(p1: Point2D, p2: Point2D, bulge: f64) -> String {
    if bulge.abs() < 1e-6 {
        return format!("{:.4} {:.4} l\n", p2.x, p2.y);
    }

    let dx = p2.x - p1.x;
    let dy = p2.y - p1.y;
    let d = dx.hypot(dy);
    if d < 1e-6 {
        return String::new();
    }

    let a = 4.0 * bulge.atan(); // included angle
    let r = (d / (2.0 * (a / 2.0).sin().abs())).abs();

    // Sagitta / distance from chord midpoint to arc center
    let s = bulge;
    let cx = (p1.x + p2.x) / 2.0 - (p2.y - p1.y) / (2.0 * s);
    let cy = (p1.y + p2.y) / 2.0 + (p2.x - p1.x) / (2.0 * s);

    let start_angle = (p1.y - cy).atan2(p1.x - cx);
    let mut end_angle = (p2.y - cy).atan2(p2.x - cx);

    if bulge > 0.0 {
        // Counter-clockwise
        while end_angle <= start_angle {
            end_angle += 2.0 * PI;
        }
    } else {
        // Clockwise
        while end_angle >= start_angle {
            end_angle -= 2.0 * PI;
        }
    }

    let total_angle = (end_angle - start_angle).abs();
    let num_segments = ((total_angle / (PI / 2.0)).ceil() as usize).max(1);
    let step = (end_angle - start_angle) / (num_segments as f64);

    let mut out = String::new();
    for i in 0..num_segments {
        let a1 = start_angle + (i as f64) * step;
        let a2 = a1 + step;
        let da = a2 - a1;

        let k = (4.0 / 3.0) * (da / 4.0).tan();

        let cos1 = a1.cos();
        let sin1 = a1.sin();
        let cos2 = a2.cos();
        let sin2 = a2.sin();

        let cp1_x = cx + r * (cos1 - k * sin1);
        let cp1_y = cy + r * (sin1 + k * cos1);
        let cp2_x = cx + r * (cos2 + k * sin2);
        let cp2_y = cy + r * (sin2 - k * cos2);
        let ep_x = cx + r * cos2;
        let ep_y = cy + r * sin2;

        out.push_str(&format!(
            "{:.4} {:.4} {:.4} {:.4} {:.4} {:.4} c\n",
            cp1_x, cp1_y, cp2_x, cp2_y, ep_x, ep_y
        ));
    }

    out
}

/// Generates PDF path operations for an ellipse or elliptical arc.
pub fn generate_ellipse_path(
    cx: f64,
    cy: f64,
    major_x: f64,
    major_y: f64,
    ratio: f64,
    start_param: f64,
    end_param: f64,
) -> String {
    let r = ratio.abs().clamp(1e-6, 1.0);
    // Major axis vector
    let mx = major_x;
    let my = major_y;
    // Minor axis vector (perpendicular to major axis, scaled by ratio)
    let nx = -major_y * r;
    let ny = major_x * r;

    let mut start = start_param;
    let mut end = end_param;
    while start < 0.0 {
        start += 2.0 * PI;
    }
    while end <= start {
        end += 2.0 * PI;
    }

    let total_angle = end - start;
    let num_segments = ((total_angle / (PI / 2.0)).ceil() as usize).max(1);
    let step = total_angle / (num_segments as f64);

    let mut out = String::new();
    let pt = |t: f64| (cx + mx * t.cos() + nx * t.sin(), cy + my * t.cos() + ny * t.sin());
    let deriv = |t: f64| (-mx * t.sin() + nx * t.cos(), -my * t.sin() + ny * t.cos());

    let (p0_x, p0_y) = pt(start);
    out.push_str(&format!("{:.4} {:.4} m\n", p0_x, p0_y));

    for i in 0..num_segments {
        let t1 = start + (i as f64) * step;
        let t2 = t1 + step;
        let dt = step;

        let k = (4.0 / 3.0) * (dt / 4.0).tan();

        let (p1x, p1y) = pt(t1);
        let (d1x, d1y) = deriv(t1);
        let (p2x, p2y) = pt(t2);
        let (d2x, d2y) = deriv(t2);

        let cp1_x = p1x + k * d1x;
        let cp1_y = p1y + k * d1y;
        let cp2_x = p2x - k * d2x;
        let cp2_y = p2y - k * d2y;

        out.push_str(&format!(
            "{:.4} {:.4} {:.4} {:.4} {:.4} {:.4} c\n",
            cp1_x, cp1_y, cp2_x, cp2_y, p2x, p2y
        ));
    }

    if (total_angle - 2.0 * PI).abs() < 1e-4 {
        out.push_str("h\n");
    }

    out
}
