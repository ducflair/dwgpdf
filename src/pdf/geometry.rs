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
        return format!("{:.2} {:.2} m\n", cx, cy);
    }
    // kappa = 4 * (sqrt(2) - 1) / 3
    let k = 0.5522847498307936 * r;

    format!(
        "{:.2} {:.2} m\n\
         {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n\
         {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n\
         {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n\
         {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n\
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
    generate_arc_path_with_mode(cx, cy, radius, start_angle, end_angle, true, true)
}

/// Generates PDF path operations for a circular arc with explicit moveto and direction controls.
pub fn generate_arc_path_with_mode(
    cx: f64,
    cy: f64,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    is_first: bool,
    counter_clockwise: bool,
) -> String {
    let r = radius.abs();
    if r < 1e-6 {
        return if is_first {
            format!("{:.2} {:.2} m\n", cx, cy)
        } else {
            format!("{:.2} {:.2} l\n", cx, cy)
        };
    }

    let start = start_angle;
    let mut end = end_angle;

    if counter_clockwise {
        while end <= start {
            end += 2.0 * PI;
        }
        while end > start + 2.0 * PI {
            end -= 2.0 * PI;
        }
    } else {
        while end >= start {
            end -= 2.0 * PI;
        }
        while end < start - 2.0 * PI {
            end += 2.0 * PI;
        }
    }

    let total_angle = (end - start).abs();
    if total_angle >= 2.0 * PI - 1e-5 {
        return if is_first {
            generate_circle_path(cx, cy, r)
        } else {
            let k = 0.5522847498307936 * r;
            format!(
                "{:.2} {:.2} l\n\
                 {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n\
                 {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n\
                 {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n\
                 {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n",
                cx + r, cy,
                cx + r, cy + k, cx + k, cy + r, cx, cy + r,
                cx - k, cy + r, cx - r, cy + k, cx - r, cy,
                cx - r, cy - k, cx - k, cy - r, cx, cy - r,
                cx + k, cy - r, cx + r, cy - k, cx + r, cy
            )
        };
    }

    // Number of segments (each <= 90 degrees = PI/2)
    let num_segments = ((total_angle / (PI / 2.0)).ceil() as usize).max(1);
    let step = (end - start) / (num_segments as f64);

    let mut out = String::new();
    let p0_x = cx + r * start.cos();
    let p0_y = cy + r * start.sin();
    if is_first {
        out.push_str(&format!("{:.2} {:.2} m\n", p0_x, p0_y));
    } else {
        out.push_str(&format!("{:.2} {:.2} l\n", p0_x, p0_y));
    }

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
            "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n",
            cp1_x, cp1_y, cp2_x, cp2_y, ep_x, ep_y
        ));
    }

    out
}

/// Generates PDF path for a bulged segment from p1 to p2.
/// In DXF: bulge = tan(included_angle / 4).
pub fn generate_bulge_segment(p1: Point2D, p2: Point2D, bulge: f64) -> String {
    if bulge.abs() < 1e-6 {
        return format!("{:.2} {:.2} l\n", p2.x, p2.y);
    }

    let dx = p2.x - p1.x;
    let dy = p2.y - p1.y;
    let d = dx.hypot(dy);
    if d < 1e-6 {
        return String::new();
    }

    let a = 4.0 * bulge.atan(); // included angle
    let r = (d / (2.0 * (a / 2.0).sin().abs())).abs();

    // Distance from chord midpoint to arc center along normal: h/d = (1 - b^2) / (4*b)
    let k = (1.0 - bulge * bulge) / (4.0 * bulge);
    let cx = (p1.x + p2.x) / 2.0 - dy * k;
    let cy = (p1.y + p2.y) / 2.0 + dx * k;

    let start_angle = (p1.y - cy).atan2(p1.x - cx);
    let sweep = 4.0 * bulge.atan();

    let total_angle = sweep.abs();
    let num_segments = ((total_angle / (PI / 2.0)).ceil() as usize).max(1);
    let step = sweep / (num_segments as f64);

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
            "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n",
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
    generate_ellipse_path_with_mode(cx, cy, major_x, major_y, ratio, start_param, end_param, true, true)
}

/// Generates PDF path operations for an ellipse or elliptical arc with explicit moveto and direction controls.
pub fn generate_ellipse_path_with_mode(
    cx: f64,
    cy: f64,
    major_x: f64,
    major_y: f64,
    ratio: f64,
    start_param: f64,
    end_param: f64,
    is_first: bool,
    counter_clockwise: bool,
) -> String {
    let r = ratio.abs().clamp(1e-6, 1.0);
    // Major axis vector
    let mx = major_x;
    let my = major_y;
    // Minor axis vector (perpendicular to major axis, scaled by ratio)
    let nx = -major_y * r;
    let ny = major_x * r;

    let start = start_param;
    let mut end = end_param;

    if counter_clockwise {
        while end <= start {
            end += 2.0 * PI;
        }
        while end > start + 2.0 * PI {
            end -= 2.0 * PI;
        }
    } else {
        while end >= start {
            end -= 2.0 * PI;
        }
        while end < start - 2.0 * PI {
            end += 2.0 * PI;
        }
    }

    let total_angle = (end - start).abs();
    let num_segments = ((total_angle / (PI / 2.0)).ceil() as usize).max(1);
    let step = (end - start) / (num_segments as f64);

    let mut out = String::new();
    let pt = |t: f64| (cx + mx * t.cos() + nx * t.sin(), cy + my * t.cos() + ny * t.sin());
    let deriv = |t: f64| (-mx * t.sin() + nx * t.cos(), -my * t.sin() + ny * t.cos());

    let (p0_x, p0_y) = pt(start);
    if is_first {
        out.push_str(&format!("{:.2} {:.2} m\n", p0_x, p0_y));
    } else {
        out.push_str(&format!("{:.2} {:.2} l\n", p0_x, p0_y));
    }

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
            "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n",
            cp1_x, cp1_y, cp2_x, cp2_y, p2x, p2y
        ));
    }

    if is_first && (total_angle - 2.0 * PI).abs() < 1e-4 {
        out.push_str("h\n");
    }

    out
}

/// Evaluates a B-spline curve at parameter `u` using De Boor's algorithm.
pub fn evaluate_bspline(
    degree: usize,
    knots: &[f64],
    control_points: &[Point2D],
    weights: Option<&[f64]>,
    u: f64,
) -> Point2D {
    let p = degree;
    let n = control_points.len().saturating_sub(1);
    let m = knots.len().saturating_sub(1);
    if p == 0 || control_points.is_empty() || knots.len() < p + 1 || m < p {
        return control_points.first().copied().unwrap_or(Point2D::new(0.0, 0.0));
    }

    let u_min = knots[p];
    let u_max = knots[m - p];
    let u_clamped = u.clamp(u_min, u_max);

    // Find knot span k such that knots[k] <= u < knots[k+1]
    let mut k = p;
    if u_clamped >= u_max {
        k = m - p - 1;
        while k > p && (knots[k] - knots[k + 1]).abs() < 1e-12 {
            k -= 1;
        }
    } else {
        while k < m - p && !(knots[k] <= u_clamped && u_clamped < knots[k + 1]) {
            k += 1;
        }
    }

    // Initialize d array of size p + 1 with points in span [k - p, k]
    let mut d = Vec::with_capacity(p + 1);
    for j in 0..=p {
        let idx = (k + j).saturating_sub(p).min(n);
        let pt = control_points[idx];
        let w = weights.and_then(|ws| ws.get(idx)).copied().unwrap_or(1.0);
        d.push([pt.x * w, pt.y * w, w]);
    }

    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = k + j - p;
            let denom = knots.get(i + p + 1 - r).copied().unwrap_or(0.0) - knots.get(i).copied().unwrap_or(0.0);
            let alpha = if denom.abs() < 1e-12 {
                0.0
            } else {
                (u_clamped - knots[i]) / denom
            };
            for c in 0..3 {
                d[j][c] = (1.0 - alpha) * d[j - 1][c] + alpha * d[j][c];
            }
        }
    }

    let w_final = d[p][2];
    if w_final.abs() < 1e-9 {
        Point2D::new(d[p][0], d[p][1])
    } else {
        Point2D::new(d[p][0] / w_final, d[p][1] / w_final)
    }
}

/// Samples a B-spline curve evenly along its valid knot domain.
pub fn sample_bspline(
    degree: usize,
    knots: &[f64],
    control_points: &[Point2D],
    weights: Option<&[f64]>,
    num_samples: usize,
) -> Vec<Point2D> {
    if knots.len() < degree + 1 || control_points.is_empty() {
        return control_points.to_vec();
    }
    let p = degree;
    let m = knots.len() - 1;
    if m < p {
        return control_points.to_vec();
    }
    let u_min = knots[p];
    let u_max = knots[m - p];
    if (u_max - u_min).abs() < 1e-12 {
        return control_points.to_vec();
    }
    let samples = num_samples.max(2);
    let mut pts = Vec::with_capacity(samples);
    for i in 0..samples {
        let t = i as f64 / (samples - 1) as f64;
        let u = u_min + t * (u_max - u_min);
        pts.push(evaluate_bspline(degree, knots, control_points, weights, u));
    }
    pts
}
