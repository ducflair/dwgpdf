use crate::error::{ConversionError, ConversionResult};
use crate::options::ConversionOptions;
use hipdf::images::{Image, ImageManager};
use hipdf::lopdf::content::{Content, Operation};
use hipdf::lopdf::{Dictionary, Document, Object, Stream};
use roxmltree::Document as XmlDocument;
use std::io::{Cursor, Read};
use zip::ZipArchive;

#[derive(Debug, Clone)]
enum DwfPrimitive {
    Line {
        p1: (f64, f64),
        p2: (f64, f64),
        color: (u8, u8, u8),
        weight: f32,
    },
    Polyline {
        pts: Vec<(f64, f64)>,
        color: (u8, u8, u8),
        fill_color: Option<(u8, u8, u8)>,
        weight: f32,
    },
    Text {
        pos: (f64, f64),
        size: f32,
        color: (u8, u8, u8),
        content: String,
    },
    Image {
        bounds: (f64, f64, f64, f64),
        bytes: Vec<u8>,
        ext: String,
    },
}

#[derive(Debug)]
#[allow(dead_code)]
struct DwfSheet {
    title: String,
    primitives: Vec<DwfPrimitive>,
    fallback_image: Option<(Vec<u8>, String)>, // For 3D or raster-only DWF
    paper_size_pt: Option<(f64, f64)>,
    is_inch: bool,
    view_bounds: Option<(f64, f64, f64, f64)>,
    transform: Option<[f64; 16]>,
}

fn normalize_image_for_pdf(raw_bytes: &[u8]) -> (Vec<u8>, String) {
    if let Ok(dyn_img) = image::load_from_memory(raw_bytes) {
        let rgba = dyn_img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let mut rgb = image::RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let p = rgba.get_pixel(x, y);
                let a = p[3] as u32;
                let r = ((p[0] as u32 * a + 255 * (255 - a)) / 255) as u8;
                let g = ((p[1] as u32 * a + 255 * (255 - a)) / 255) as u8;
                let b = ((p[2] as u32 * a + 255 * (255 - a)) / 255) as u8;
                rgb.put_pixel(x, y, image::Rgb([r, g, b]));
            }
        }
        let mut out = Cursor::new(Vec::new());
        if rgb.write_to(&mut out, image::ImageFormat::Png).is_ok() {
            return (out.into_inner(), "png".to_string());
        }
    }
    (raw_bytes.to_vec(), "png".to_string())
}

fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some((r, g, b))
    } else if s.len() == 3 {
        let r = u8::from_str_radix(&s[0..1], 16).ok()? * 17;
        let g = u8::from_str_radix(&s[1..2], 16).ok()? * 17;
        let b = u8::from_str_radix(&s[2..3], 16).ok()? * 17;
        Some((r, g, b))
    } else {
        None
    }
}

/// Parse a textual W2D ASCII stream (e.g. text-w2d.dwf).
fn parse_textual_w2d(content: &str) -> Vec<DwfPrimitive> {
    let mut primitives = Vec::new();
    let mut current_color = (0u8, 0u8, 0u8);
    let mut current_fill: Option<(u8, u8, u8)> = None;
    let mut current_weight = 1.0f32;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        let cmd = match parts.next() {
            Some(c) => c.to_lowercase(),
            None => continue,
        };

        match cmd.as_str() {
            "color" => {
                if let Some(val) = parts.next() {
                    if let Some(col) = parse_hex_color(val) {
                        current_color = col;
                    }
                }
            }
            "fill" => {
                if let Some(val) = parts.next() {
                    if val.eq_ignore_ascii_case("none") {
                        current_fill = None;
                    } else if let Some(col) = parse_hex_color(val) {
                        current_fill = Some(col);
                    }
                }
            }
            "lineweight" => {
                if let Some(val) = parts.next() {
                    if let Ok(w) = val.parse::<f32>() {
                        current_weight = w.max(0.5);
                    }
                }
            }
            "polyline" | "polygon" => {
                let is_polygon = cmd == "polygon";
                let coords: Vec<f64> = parts.filter_map(|s| s.parse::<f64>().ok()).collect();
                let mut pts = Vec::new();
                for chunk in coords.chunks(2) {
                    if chunk.len() == 2 {
                        pts.push((chunk[0], chunk[1]));
                    }
                }
                if pts.len() >= 2 {
                    primitives.push(DwfPrimitive::Polyline {
                        pts,
                        color: current_color,
                        fill_color: if is_polygon { current_fill } else { None },
                        weight: current_weight,
                    });
                }
            }
            "path" => {
                // Parse SVG-like path: M x y C x1 y1 x2 y2 x3 y3 ... Z
                let rest: Vec<&str> = parts.collect();
                let mut i = 0;
                let mut path_pts = Vec::new();
                while i < rest.len() {
                    let token = rest[i];
                    match token {
                        "M" | "L" => {
                            if i + 2 < rest.len() {
                                if let (Ok(x), Ok(y)) = (rest[i + 1].parse::<f64>(), rest[i + 2].parse::<f64>()) {
                                    path_pts.push((x, y));
                                }
                                i += 3;
                            } else {
                                break;
                            }
                        }
                        "C" => {
                            // Cubic Bezier - approximate with endpoints and midpoints
                            if i + 6 < rest.len() {
                                if let (Ok(x1), Ok(y1), Ok(x2), Ok(y2), Ok(x3), Ok(y3)) = (
                                    rest[i + 1].parse::<f64>(),
                                    rest[i + 2].parse::<f64>(),
                                    rest[i + 3].parse::<f64>(),
                                    rest[i + 4].parse::<f64>(),
                                    rest[i + 5].parse::<f64>(),
                                    rest[i + 6].parse::<f64>(),
                                ) {
                                    let mid_x = 0.25 * x1 + 0.5 * x2 + 0.25 * x3;
                                    let mid_y = 0.25 * y1 + 0.5 * y2 + 0.25 * y3;
                                    path_pts.push((mid_x, mid_y));
                                    path_pts.push((x3, y3));
                                }
                                i += 7;
                            } else {
                                break;
                            }
                        }
                        "Z" => {
                            if let Some(&first) = path_pts.first() {
                                path_pts.push(first);
                            }
                            i += 1;
                        }
                        _ => {
                            i += 1;
                        }
                    }
                }
                if path_pts.len() >= 2 {
                    primitives.push(DwfPrimitive::Polyline {
                        pts: path_pts,
                        color: current_color,
                        fill_color: current_fill,
                        weight: current_weight,
                    });
                }
            }
            "text" => {
                let coords: Vec<&str> = parts.collect();
                if coords.len() >= 3 {
                    let x = coords[0].parse::<f64>().unwrap_or(0.0);
                    let y = coords[1].parse::<f64>().unwrap_or(0.0);
                    let size = coords[2].parse::<f32>().unwrap_or(12.0);
                    let content_str = coords[3..].join(" ").trim_matches('"').to_string();
                    if !content_str.is_empty() {
                        primitives.push(DwfPrimitive::Text {
                            pos: (x, y),
                            size,
                            color: current_color,
                            content: content_str,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    primitives
}

/// Parse an uncompressed legacy ASCII DWF (DWF V00.xx).
fn parse_legacy_ascii_dwf(content: &str) -> Vec<DwfPrimitive> {
    let mut primitives = Vec::new();
    let mut colormap: Vec<(u8, u8, u8)> = Vec::new();
    let mut current_color = (0u8, 0u8, 0u8);
    let mut current_fill: Option<(u8, u8, u8)> = None;

    let lines: Vec<&str> = content.lines().collect();
    let mut line_idx = 0;

    while line_idx < lines.len() {
        let trimmed = lines[line_idx].trim();
        line_idx += 1;

        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("(ColorMap") {
            // Read colormap entries until closing parenthesis
            let mut map_str = trimmed.to_string();
            while !map_str.contains(')') && line_idx < lines.len() {
                map_str.push(' ');
                map_str.push_str(lines[line_idx].trim());
                line_idx += 1;
            }
            // Parse RGBA entries like 0,0,0,255 255,255,255,255 ...
            for token in map_str.split_whitespace() {
                let parts: Vec<&str> = token.trim_matches(|c| c == '(' || c == ')').split(',').collect();
                if parts.len() >= 3 {
                    if let (Ok(r), Ok(g), Ok(b)) = (
                        parts[0].parse::<u8>(),
                        parts[1].parse::<u8>(),
                        parts[2].parse::<u8>(),
                    ) {
                        colormap.push((r, g, b));
                    }
                }
            }
            continue;
        }

        if trimmed.starts_with("C ") {
            let idx_str = trimmed.trim_start_matches("C ").trim();
            if let Ok(idx) = idx_str.parse::<usize>() {
                if let Some(&col) = colormap.get(idx) {
                    // In CAD on white background, pure white color is plotted as black
                    current_color = if col == (255, 255, 255) { (0, 0, 0) } else { col };
                }
            }
            continue;
        }

        if trimmed.starts_with("L ") {
            // L x1,y1 x2,y2
            let rest = trimmed.trim_start_matches("L ").trim();
            let pts: Vec<&str> = rest.split_whitespace().collect();
            if pts.len() >= 2 {
                let p1_parts: Vec<&str> = pts[0].split(',').collect();
                let p2_parts: Vec<&str> = pts[1].split(',').collect();
                if p1_parts.len() == 2 && p2_parts.len() == 2 {
                    if let (Ok(x1), Ok(y1), Ok(x2), Ok(y2)) = (
                        p1_parts[0].parse::<f64>(),
                        p1_parts[1].parse::<f64>(),
                        p2_parts[0].parse::<f64>(),
                        p2_parts[1].parse::<f64>(),
                    ) {
                        primitives.push(DwfPrimitive::Line {
                            p1: (x1, y1),
                            p2: (x2, y2),
                            color: current_color,
                            weight: 1.0,
                        });
                    }
                }
            }
            continue;
        }

        if trimmed.starts_with("P ") {
            // P count x1,y1 x2,y2 ...
            let mut p_str = trimmed.to_string();
            let mut tokens: Vec<&str> = p_str.split_whitespace().collect();
            let expected_cnt = tokens.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);

            while tokens.len() < expected_cnt + 2 && line_idx < lines.len() {
                let next_line = lines[line_idx].trim();
                line_idx += 1;
                p_str.push(' ');
                p_str.push_str(next_line);
                tokens = p_str.split_whitespace().collect();
            }

            let mut pts = Vec::new();
            for &token in &tokens[2..] {
                let pair: Vec<&str> = token.split(',').collect();
                if pair.len() == 2 {
                    if let (Ok(x), Ok(y)) = (pair[0].parse::<f64>(), pair[1].parse::<f64>()) {
                        pts.push((x, y));
                    }
                }
            }

            if pts.len() >= 2 {
                primitives.push(DwfPrimitive::Polyline {
                    pts,
                    color: current_color,
                    fill_color: current_fill,
                    weight: 1.0,
                });
            }
            continue;
        }

        if trimmed == "f" || trimmed == "F" {
            current_fill = Some(current_color);
            continue;
        }
    }

    primitives
}

fn read_w2d_count(data: &[u8], idx: &mut usize) -> Option<usize> {
    if *idx >= data.len() {
        return None;
    }
    let b = data[*idx];
    *idx += 1;
    if b != 0 {
        Some(b as usize)
    } else {
        if *idx + 2 > data.len() {
            return None;
        }
        let tmp = u16::from_le_bytes([data[*idx], data[*idx + 1]]) as usize;
        *idx += 2;
        Some(256 + tmp)
    }
}

fn clean_dwf_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                if next == 'P' {
                    chars.next();
                    out.push('\n');
                    continue;
                } else if next == '~' {
                    chars.next();
                    out.push(' ');
                    continue;
                } else if next == '\\' || next == '{' || next == '}' {
                    chars.next();
                    out.push(next);
                    continue;
                } else if next == 'L' || next == 'l' || next == 'O' || next == 'o' || next == 'K' || next == 'k' {
                    chars.next();
                    continue;
                } else {
                    let mut is_cmd = false;
                    let mut cmd_buf = String::new();
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == ';' {
                            is_cmd = true;
                            break;
                        }
                        cmd_buf.push(c);
                        if cmd_buf.len() > 40 {
                            break;
                        }
                    }
                    if !is_cmd {
                        out.push('\\');
                        out.push_str(&cmd_buf);
                    }
                    continue;
                }
            }
        } else if ch == '{' || ch == '}' {
            continue;
        } else {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn read_w2d_string(data: &[u8], idx: &mut usize) -> String {
    while *idx < data.len() && (data[*idx] == b' ' || data[*idx] == b'\t' || data[*idx] == b'\r' || data[*idx] == b'\n') {
        *idx += 1;
    }
    if *idx >= data.len() {
        return String::new();
    }
    let lead = data[*idx];
    if lead == b'{' {
        *idx += 1;
        if *idx + 4 > data.len() {
            return String::new();
        }
        let len = u32::from_le_bytes([data[*idx], data[*idx + 1], data[*idx + 2], data[*idx + 3]]) as usize;
        *idx += 4;
        let char_bytes = len * 2;
        let mut u16_chars = Vec::with_capacity(len);
        for i in 0..len {
            let p = *idx + i * 2;
            if p + 2 <= data.len() {
                u16_chars.push(u16::from_le_bytes([data[p], data[p + 1]]));
            }
        }
        *idx += char_bytes.min(data.len() - *idx);
        if *idx < data.len() && data[*idx] == b'}' {
            *idx += 1;
        }
        clean_dwf_text(&char::decode_utf16(u16_chars).map(|r| r.unwrap_or('\u{FFFD}')).collect::<String>())
    } else if lead == b'\'' || lead == b'"' {
        let quote = lead;
        *idx += 1;
        let start = *idx;
        while *idx < data.len() && data[*idx] != quote {
            *idx += 1;
        }
        let s = String::from_utf8_lossy(&data[start..*idx]).to_string();
        if *idx < data.len() {
            *idx += 1;
        }
        clean_dwf_text(&s)
    } else {
        let start = *idx;
        while *idx < data.len() && data[*idx] != b' ' && data[*idx] != b'\t' && data[*idx] != b'\r' && data[*idx] != b'\n' && data[*idx] != b')' {
            *idx += 1;
        }
        clean_dwf_text(&String::from_utf8_lossy(&data[start..*idx]))
    }
}

/// Parse binary W2D streams (starts with `(W2D V06.xx)`).
fn parse_binary_w2d(data: &[u8]) -> (Vec<DwfPrimitive>, Option<(f64, f64)>, Option<(f64, f64, f64, f64)>) {
    let mut primitives = Vec::new();
    let mut cur_x = 0i64;
    let mut cur_y = 0i64;
    let mut cur_color = (0u8, 0u8, 0u8);
    let mut cur_weight = 1.0f32;
    let mut cur_font_size = 150.0f32;
    let mut paper_size_override = None;
    let mut view_bounds_override = None;
    let mut idx = 0;

    while idx < data.len() {
        let b = data[idx];
        if b == b'\r' || b == b'\n' || b == b' ' || b == b'\t' || b == 0 {
            idx += 1;
            continue;
        }

        // Extended ASCII: (Opcode ...)
        if b == b'(' {
            idx += 1;
            let start = idx;
            let mut depth = 1;
            while idx < data.len() && depth > 0 {
                let c = data[idx];
                if c == b'(' {
                    depth += 1;
                    idx += 1;
                } else if c == b')' {
                    depth -= 1;
                    idx += 1;
                } else if c == b'\'' || c == b'"' {
                    let q = c;
                    idx += 1;
                    while idx < data.len() && data[idx] != q {
                        idx += 1;
                    }
                    if idx < data.len() {
                        idx += 1;
                    }
                } else if c == b'{' {
                    idx += 1;
                    if idx + 4 <= data.len() {
                        let len = u32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]) as usize;
                        idx += 4 + len;
                        if idx < data.len() && data[idx] == b'}' {
                            idx += 1;
                        }
                    }
                } else {
                    idx += 1;
                }
            }

            let cmd_bytes = &data[start..idx.saturating_sub(1)];
            if let Ok(cmd_str) = std::str::from_utf8(cmd_bytes) {
                if cmd_str.starts_with("PlotInfo show ") {
                    let parts: Vec<&str> = cmd_str["PlotInfo show ".len()..].split_whitespace().collect();
                    if parts.len() >= 4 {
                        let units = parts[1];
                        let pw = parts[2].parse::<f64>().ok();
                        let ph = parts[3].parse::<f64>().ok();
                        if let (Some(w), Some(h)) = (pw, ph) {
                            let (w_pt, h_pt) = if units.eq_ignore_ascii_case("in") || units.eq_ignore_ascii_case("inches") {
                                (w * 72.0, h * 72.0)
                            } else {
                                (w * 72.0 / 25.4, h * 72.0 / 25.4)
                            };
                            if w_pt > 10.0 && h_pt > 10.0 {
                                paper_size_override = Some((w_pt, h_pt));
                            }
                        }
                    }
                } else if cmd_str.starts_with("View ") {
                    let parts: Vec<&str> = cmd_str["View ".len()..].split_whitespace().collect();
                    if parts.len() >= 2 {
                        let p1: Vec<&str> = parts[0].split(',').collect();
                        let p2: Vec<&str> = parts[1].split(',').collect();
                        if p1.len() == 2 && p2.len() == 2 {
                            if let (Ok(x1), Ok(y1), Ok(x2), Ok(y2)) = (
                                p1[0].parse::<f64>(),
                                p1[1].parse::<f64>(),
                                p2[0].parse::<f64>(),
                                p2[1].parse::<f64>(),
                            ) {
                                view_bounds_override = Some((x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2)));
                            }
                        }
                    }
                }
            }
            continue;
        }

        // Extended Binary: { <len: u32> ... }
        if b == b'{' {
            idx += 1;
            if idx + 4 <= data.len() {
                let len = u32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]) as usize;
                idx += 4;
                let end = (idx + len).min(data.len());
                let block = &data[idx..end];
                if block.len() >= 22 {
                    let dx = i32::from_le_bytes([block[6], block[7], block[8], block[9]]) as f64;
                    let dy = i32::from_le_bytes([block[10], block[11], block[12], block[13]]) as f64;
                    let w = (i32::from_le_bytes([block[14], block[15], block[16], block[17]]) as f64).abs();
                    let h = (i32::from_le_bytes([block[18], block[19], block[20], block[21]]) as f64).abs();
                    let abs_x = cur_x as f64 + dx;
                    let abs_y = cur_y as f64 + dy;
                    cur_x = (abs_x + w) as i64;
                    cur_y = (abs_y + h) as i64;

                    if let Some(pos) = block.windows(8).position(|w| w == b"\x89PNG\r\n\x1a\n") {
                        let img_bytes = block[pos..].to_vec();
                        let (norm_bytes, norm_ext) = normalize_image_for_pdf(&img_bytes);
                        primitives.push(DwfPrimitive::Image {
                            bounds: (abs_x, abs_y, w, h),
                            bytes: norm_bytes,
                            ext: norm_ext,
                        });
                    } else if let Some(pos) = block.windows(3).position(|w| w == b"\xff\xd8\xff") {
                        let img_bytes = block[pos..].to_vec();
                        let (norm_bytes, norm_ext) = normalize_image_for_pdf(&img_bytes);
                        primitives.push(DwfPrimitive::Image {
                            bounds: (abs_x, abs_y, w, h),
                            bytes: norm_bytes,
                            ext: norm_ext,
                        });
                    }
                }
                idx = end;
                if idx < data.len() && data[idx] == b'}' {
                    idx += 1;
                }
                continue;
            } else {
                break;
            }
        }

        if b == b'}' {
            idx += 1;
            continue;
        }

        match b {
            // WD_SBBO_DRAW_LINE_16R: 0x0c
            0x0c => {
                idx += 1;
                if idx + 8 <= data.len() {
                    let dx1 = i16::from_le_bytes([data[idx], data[idx + 1]]) as i64;
                    let dy1 = i16::from_le_bytes([data[idx + 2], data[idx + 3]]) as i64;
                    let dx2 = i16::from_le_bytes([data[idx + 4], data[idx + 5]]) as i64;
                    let dy2 = i16::from_le_bytes([data[idx + 6], data[idx + 7]]) as i64;
                    let p1_x = cur_x + dx1;
                    let p1_y = cur_y + dy1;
                    let p2_x = p1_x + dx2;
                    let p2_y = p1_y + dy2;
                    cur_x = p2_x;
                    cur_y = p2_y;
                    idx += 8;

                    primitives.push(DwfPrimitive::Line {
                        p1: (p1_x as f64, p1_y as f64),
                        p2: (p2_x as f64, p2_y as f64),
                        color: cur_color,
                        weight: cur_weight,
                    });
                } else {
                    break;
                }
            }

            // WD_SBBO_DRAW_LINE_32R ('l' = 0x6c)
            0x6c => {
                idx += 1;
                if idx + 16 <= data.len() {
                    let dx1 = i32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]) as i64;
                    let dy1 = i32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as i64;
                    let dx2 = i32::from_le_bytes([data[idx + 8], data[idx + 9], data[idx + 10], data[idx + 11]]) as i64;
                    let dy2 = i32::from_le_bytes([data[idx + 12], data[idx + 13], data[idx + 14], data[idx + 15]]) as i64;
                    let p1_x = cur_x + dx1;
                    let p1_y = cur_y + dy1;
                    let p2_x = p1_x + dx2;
                    let p2_y = p1_y + dy2;
                    cur_x = p2_x;
                    cur_y = p2_y;
                    idx += 16;

                    primitives.push(DwfPrimitive::Line {
                        p1: (p1_x as f64, p1_y as f64),
                        p2: (p2_x as f64, p2_y as f64),
                        color: cur_color,
                        weight: cur_weight,
                    });
                } else {
                    break;
                }
            }

            // WD_SBBO_DRAW_CIRCULAR_ARC_32R: 0x92
            0x92 => {
                idx += 1;
                if idx + 16 <= data.len() {
                    let dx = i32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]) as i64;
                    let dy = i32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as i64;
                    let r = i32::from_le_bytes([data[idx + 8], data[idx + 9], data[idx + 10], data[idx + 11]]) as f64;
                    if r > 3000.0 {
                        eprintln!("Large circular arc 0x92: r={}, cur=({}, {}), dx={}, dy={}", r, cur_x, cur_y, dx, dy);
                    }
                    let start_ang = u16::from_le_bytes([data[idx + 12], data[idx + 13]]);
                    let end_ang = u16::from_le_bytes([data[idx + 14], data[idx + 15]]);
                    let cx = (cur_x + dx) as f64;
                    let cy = (cur_y + dy) as f64;
                    cur_x += dx;
                    cur_y += dy;
                    idx += 16;

                    let sa = (start_ang as f64 / 65536.0) * 2.0 * std::f64::consts::PI;
                    let mut ea = (end_ang as f64 / 65536.0) * 2.0 * std::f64::consts::PI;
                    if ea <= sa {
                        ea += 2.0 * std::f64::consts::PI;
                    }
                    let steps = 16;
                    let mut pts = Vec::with_capacity(steps + 1);
                    for i in 0..=steps {
                        let t = sa + (ea - sa) * (i as f64 / steps as f64);
                        pts.push((cx + r * t.cos(), cy + r * t.sin()));
                    }
                    primitives.push(DwfPrimitive::Polyline {
                        pts,
                        color: cur_color,
                        fill_color: None,
                        weight: cur_weight,
                    });
                } else {
                    break;
                }
            }

            // WD_SBBO_DRAW_POLYLINE_POLYGON_16R (0x10), 0x14, 0x15, or 0x8d
            0x10 | 0x14 | 0x15 | 0x8d => {
                let is_polygon = b == 0x14 || b == 0x15;
                idx += 1;
                if let Some(cnt) = read_w2d_count(data, &mut idx) {
                    let mut pts = Vec::with_capacity(cnt);
                    for _ in 0..cnt {
                        if idx + 4 <= data.len() {
                            let dx = i16::from_le_bytes([data[idx], data[idx + 1]]) as i64;
                            let dy = i16::from_le_bytes([data[idx + 2], data[idx + 3]]) as i64;
                            cur_x += dx;
                            cur_y += dy;
                            pts.push((cur_x as f64, cur_y as f64));
                            idx += 4;
                        } else {
                            break;
                        }
                    }
                    if pts.len() >= 2 {
                        primitives.push(DwfPrimitive::Polyline {
                            pts,
                            color: cur_color,
                            fill_color: if is_polygon { Some(cur_color) } else { None },
                            weight: cur_weight,
                        });
                    }
                } else {
                    break;
                }
            }

            // WD_SBBO_DRAW_POLYLINE_POLYGON_32R ('p' = 0x70) or WD_SBAO_DRAW_POLYLINE_POLYGON ('P' = 0x50)
            0x70 | 0x50 => {
                let is_polygon = b == 0x50;
                idx += 1;
                if let Some(cnt) = read_w2d_count(data, &mut idx) {
                    let mut pts = Vec::with_capacity(cnt);
                    for _ in 0..cnt {
                        if idx + 8 <= data.len() {
                            let dx = i32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]) as i64;
                            let dy = i32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as i64;
                            cur_x += dx;
                            cur_y += dy;
                            pts.push((cur_x as f64, cur_y as f64));
                            idx += 8;
                        } else {
                            break;
                        }
                    }
                    if pts.len() >= 2 {
                        primitives.push(DwfPrimitive::Polyline {
                            pts,
                            color: cur_color,
                            fill_color: if is_polygon { Some(cur_color) } else { None },
                            weight: cur_weight,
                        });
                    }
                } else {
                    break;
                }
            }

            // WD_SBBO_SET_ORIGIN_32 ('O' = 0x4f)
            0x4f => {
                idx += 1;
                if idx + 8 <= data.len() {
                    cur_x = i32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]) as i64;
                    cur_y = i32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as i64;
                    idx += 8;
                } else {
                    break;
                }
            }

            // WD_SBBO_SET_COLOR_RGBA: 0x03
            0x03 => {
                idx += 1;
                if idx + 4 <= data.len() {
                    let r = data[idx];
                    let g = data[idx + 1];
                    let b_col = data[idx + 2];
                    cur_color = if (r, g, b_col) == (255, 255, 255) { (0, 0, 0) } else { (r, g, b_col) };
                    idx += 4;
                } else {
                    break;
                }
            }

            // WD_SBBO_SET_COLOR_INDEXED: 'c' = 0x63
            0x63 => {
                idx += 1;
                if idx < data.len() {
                    idx += 1;
                }
            }

            // WD_SBBO_SET_LINE_WEIGHT: 0x17
            0x17 => {
                idx += 1;
                if idx + 4 <= data.len() {
                    let w = u32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]);
                    cur_weight = (w as f32 * 0.05).max(0.5);
                    idx += 4;
                } else {
                    break;
                }
            }

            // WD_SBBO_SET_LAYER: 0xac
            0xac => {
                idx += 1;
                if idx < data.len() {
                    idx += 1;
                }
            }

            // 1-byte opcodes (fill, visibility, auto node)
            0x0e | 0x46 | 0x66 | 0x56 | 0x76 => {
                idx += 1;
            }

            // 16-bit node: 'n' = 0x6e
            0x6e => {
                idx += 3;
            }

            // 32-bit node: 'N' = 0x4e
            0x4e => {
                idx += 5;
            }

            // WD_SBBO_DRAW_ELLIPSE_32R ('e' = 0x65) or WD_SBAO_DRAW_ELLIPSE ('E' = 0x45)
            0x65 | 0x45 => {
                idx += 1;
                if idx + 22 <= data.len() {
                    let dx = i32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]) as i64;
                    let dy = i32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as i64;
                    let major = i32::from_le_bytes([data[idx + 8], data[idx + 9], data[idx + 10], data[idx + 11]]) as f64;
                    let minor = i32::from_le_bytes([data[idx + 12], data[idx + 13], data[idx + 14], data[idx + 15]]) as f64;
                    if major > 3000.0 || minor > 3000.0 {
                        eprintln!("Large ellipse 0x{:02x}: major={}, minor={}, cur=({}, {}), dx={}, dy={}", b, major, minor, cur_x, cur_y, dx, dy);
                    }
                    let tilt_raw = u16::from_le_bytes([data[idx + 20], data[idx + 21]]);
                    let tilt = (tilt_raw as f64 / 65536.0) * 2.0 * std::f64::consts::PI;
                    let cx = (cur_x + dx) as f64;
                    let cy = (cur_y + dy) as f64;
                    cur_x += dx;
                    cur_y += dy;
                    idx += 22;

                    let steps = 32;
                    let mut pts = Vec::with_capacity(steps + 1);
                    let cos_tilt = tilt.cos();
                    let sin_tilt = tilt.sin();
                    for i in 0..=steps {
                        let t = (i as f64 / steps as f64) * 2.0 * std::f64::consts::PI;
                        let ex = major * t.cos();
                        let ey = minor * t.sin();
                        let rx = ex * cos_tilt - ey * sin_tilt;
                        let ry = ex * sin_tilt + ey * cos_tilt;
                        pts.push((cx + rx, cy + ry));
                    }
                    primitives.push(DwfPrimitive::Polyline {
                        pts,
                        color: cur_color,
                        fill_color: None,
                        weight: cur_weight,
                    });
                } else {
                    break;
                }
            }

            // WD_SBBO_DRAW_CIRCLE_32R ('r' = 0x72) or 0x12
            0x72 | 0x12 => {
                idx += 1;
                if idx + 12 <= data.len() {
                    let dx = i32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]) as i64;
                    let dy = i32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as i64;
                    let r = i32::from_le_bytes([data[idx + 8], data[idx + 9], data[idx + 10], data[idx + 11]]) as f64;
                    let cx = (cur_x + dx) as f64;
                    let cy = (cur_y + dy) as f64;
                    cur_x += dx;
                    cur_y += dy;
                    idx += 12;

                    let steps = 32;
                    let mut pts = Vec::with_capacity(steps + 1);
                    for i in 0..=steps {
                        let t = (i as f64 / steps as f64) * 2.0 * std::f64::consts::PI;
                        pts.push((cx + r * t.cos(), cy + r * t.sin()));
                    }
                    primitives.push(DwfPrimitive::Polyline {
                        pts,
                        color: cur_color,
                        fill_color: None,
                        weight: cur_weight,
                    });
                } else {
                    break;
                }
            }

            // WD_SBBO_DRAW_TEXT_BASIC ('x' = 0x78)
            0x78 => {
                idx += 1;
                if idx + 8 <= data.len() {
                    let dx = i32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]) as i64;
                    let dy = i32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as i64;
                    cur_x += dx;
                    cur_y += dy;
                    idx += 8;
                    let text = read_w2d_string(data, &mut idx);
                    if !text.is_empty() {
                        primitives.push(DwfPrimitive::Text {
                            pos: (cur_x as f64, cur_y as f64),
                            size: cur_font_size,
                            color: cur_color,
                            content: text,
                        });
                    }
                } else {
                    break;
                }
            }

            // WD_SBBO_DRAW_TEXT_COMPLEX (0x18)
            0x18 => {
                idx += 1;
                if idx + 8 <= data.len() {
                    let dx = i32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]) as i64;
                    let dy = i32::from_le_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]) as i64;
                    cur_x += dx;
                    cur_y += dy;
                    idx += 8;
                    let text = read_w2d_string(data, &mut idx);
                    // 1 byte overscore + 1 byte underscore + 32 bytes bounds + 1 byte reserved = 35 bytes
                    idx += 35;
                    if !text.is_empty() {
                        primitives.push(DwfPrimitive::Text {
                            pos: (cur_x as f64, cur_y as f64),
                            size: cur_font_size,
                            color: cur_color,
                            content: text,
                        });
                    }
                } else {
                    break;
                }
            }

            // WD_SBBO_SET_FONT (0x06)
            0x06 => {
                idx += 1;
                if idx + 2 <= data.len() {
                    let fields = u16::from_le_bytes([data[idx], data[idx + 1]]);
                    idx += 2;
                    if fields & 0x0001 != 0 {
                        let _ = read_w2d_string(data, &mut idx);
                    }
                    if fields & 0x0002 != 0 { idx += 1; }
                    if fields & 0x0004 != 0 { idx += 1; }
                    if fields & 0x0008 != 0 { idx += 1; }
                    if fields & 0x0010 != 0 { idx += 1; }
                    if fields & 0x0020 != 0 {
                        let h = i32::from_le_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]);
                        cur_font_size = (h.abs() as f32).max(10.0);
                        idx += 4;
                    }
                    if fields & 0x0040 != 0 { idx += 2; }
                    if fields & 0x0080 != 0 { idx += 2; }
                    if fields & 0x0100 != 0 { idx += 2; }
                    if fields & 0x0200 != 0 { idx += 2; }
                    if fields & 0x0400 != 0 { idx += 4; }
                } else {
                    break;
                }
            }

            0x41 | 0x62 | 0x57 => {
                idx += 6;
            }
            0x11 => {
                idx += 4;
            }
            0xa0..=0xaf => {
                idx += 4;
            }

            _ => {
                idx += 1;
            }
        }
    }

    (primitives, paper_size_override, view_bounds_override)
}

fn find_file_in_archive<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    target_path: &str,
) -> Option<usize> {
    let norm1 = target_path.replace('\\', "/");
    let norm2 = target_path.replace('/', "\\");

    if let Some(idx) = archive.index_for_name(&norm1).or_else(|| archive.index_for_name(&norm2)) {
        return Some(idx);
    }

    let target_filename = norm1.rsplit('/').next().unwrap_or(&norm1).to_lowercase();
    for i in 0..archive.len() {
        if let Ok(f) = archive.by_index(i) {
            let name = f.name().to_string();
            let name_norm = name.replace('\\', "/");
            if name_norm.eq_ignore_ascii_case(&norm1) || name.eq_ignore_ascii_case(&norm2) {
                return Some(i);
            }
            if !target_filename.is_empty() && name_norm.to_lowercase().ends_with(&target_filename) {
                return Some(i);
            }
        }
    }
    None
}

/// Converts any DWF package or legacy DWF file to PDF bytes using hipdf.
pub fn convert_dwf_to_pdf(dwf_bytes: &[u8], options: &ConversionOptions) -> ConversionResult<Vec<u8>> {
    // 1. Check if it's a DWF ZIP package (contains End of Central Directory signature PK\x05\x06)
    let is_zip_package = dwf_bytes.windows(4).any(|w| w == b"PK\x05\x06");

    if is_zip_package {
        let cursor = Cursor::new(dwf_bytes);
        let mut archive = ZipArchive::new(cursor).map_err(|e| {
            ConversionError::DwfParse(format!("Failed to open DWF zip archive: {}", e))
        })?;

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
                        let section_type = node.attribute("type").unwrap_or("");
                        // In DWF, only graphical sections (ePlot, eModel, sheet) are viewable pages;
                        // Data sections (com.autodesk.dwf.Data) represent tabular metadata (BOM, properties)
                        if section_type.ends_with(".Data") {
                            continue;
                        }

                        let section_title = node
                            .attribute("title")
                            .unwrap_or("Untitled Sheet")
                            .to_string();

                        if title_from_manifest.is_none() {
                            title_from_manifest = Some(section_title.clone());
                        }

                        let mut sheet_paper_size = None;
                        let mut is_inch = false;
                        let mut sheet_view_bounds = None;
                        let mut sheet_transform = None;
                        let mut w2d_path = None;
                        let mut fallback_img = None;
                        let mut best_img_priority = 0;
                        let mut best_img_size = 0;

                        for child in node.descendants() {
                            if child.has_tag_name("Resource") {
                                let role = child.attribute("role").unwrap_or("");
                                let mime = child.attribute("mime").unwrap_or("");
                                let href = child.attribute("href").unwrap_or("");

                                // Descriptor to extract paper dimensions
                                if role == "descriptor" || mime == "application/x-descriptor" || href.ends_with("descriptor.xml") {
                                    let zip_idx = find_file_in_archive(&mut archive, href);
                                    if let Some(idx) = zip_idx {
                                        if let Ok(mut f) = archive.by_index(idx) {
                                            let mut xml_str = String::new();
                                            if f.read_to_string(&mut xml_str).is_ok() {
                                                if let Ok(desc_doc) = XmlDocument::parse(&xml_str) {
                                                    let mut paper_units = "mm".to_string();
                                                    for elem in desc_doc.descendants() {
                                                        if elem.tag_name().name() == "Paper" {
                                                            if let Some(u) = elem.attribute("units") {
                                                                paper_units = u.to_string();
                                                            }
                                                            let width = elem.attribute("width").and_then(|s| s.parse::<f64>().ok());
                                                            let height = elem.attribute("height").and_then(|s| s.parse::<f64>().ok());
                                                            if let (Some(w), Some(h)) = (width, height) {
                                                                if paper_units.eq_ignore_ascii_case("in") || paper_units.eq_ignore_ascii_case("inches") {
                                                                    is_inch = true;
                                                                }
                                                                let (w_pt, h_pt) = if is_inch {
                                                                    (w * 72.0, h * 72.0)
                                                                } else {
                                                                    (w * 72.0 / 25.4, h * 72.0 / 25.4)
                                                                };
                                                                if w_pt > 10.0 && h_pt > 10.0 {
                                                                    sheet_paper_size = Some((w_pt, h_pt));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                // Graphic resource: .w2d vector stream
                                if role == "2d streaming graphics" || role == "graphics" || mime == "application/x-w2d" || href.ends_with(".w2d") {
                                    w2d_path = Some(href.to_string());
                                    if let Some(t_str) = child.attribute("transform") {
                                        let values: Vec<f64> = t_str.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                                        if values.len() == 16 {
                                            let mut transform = [0.0; 16];
                                            transform.copy_from_slice(&values);
                                            sheet_transform = Some(transform);
                                        }
                                    }
                                }

                                // Fallback image resource (for 3D models or sheet preview).
                                // Textures are material maps, never page content; a 3D eModel's
                                // thumbnail is the correct 2D representation. Prefer the real
                                // preview/graphic image, then the thumbnail, then other images.
                                if role != "icon" && !href.is_empty() {
                                    let priority = if role == "preview" || role == "graphic" {
                                        4
                                    } else if role == "thumbnail" {
                                        3
                                    } else if mime.starts_with("image/") && role != "texture" {
                                        2
                                    } else {
                                        0
                                    };

                                    if priority > best_img_priority || (priority == best_img_priority && priority > 0) {
                                        let zip_idx = find_file_in_archive(&mut archive, href);
                                        if let Some(idx) = zip_idx {
                                            if let Ok(mut f) = archive.by_index(idx) {
                                                let mut buf = Vec::new();
                                                if f.read_to_end(&mut buf).is_ok() && buf.len() > 512 {
                                                    if priority > best_img_priority || buf.len() > best_img_size {
                                                        let (norm_bytes, norm_ext) = normalize_image_for_pdf(&buf);
                                                        fallback_img = Some((norm_bytes, norm_ext));
                                                        best_img_priority = priority;
                                                        best_img_size = buf.len();
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Try parsing W2D vector graphics if available
                        let mut sheet_primitives = Vec::new();
                        if let Some(href) = &w2d_path {
                            let zip_idx = find_file_in_archive(&mut archive, href);
                            if let Some(idx) = zip_idx {
                                if let Ok(mut f) = archive.by_index(idx) {
                                    let mut w2d_bytes = Vec::new();
                                    if f.read_to_end(&mut w2d_bytes).is_ok() {
                                        // Check if textual or binary
                                        if w2d_bytes.starts_with(b"#")
                                            || w2d_bytes.starts_with(b"color")
                                            || w2d_bytes.starts_with(b"polygon")
                                            || w2d_bytes.starts_with(b"lineweight")
                                        {
                                            let text_str = String::from_utf8_lossy(&w2d_bytes);
                                            sheet_primitives = parse_textual_w2d(&text_str);
                                        } else {
                                            let (bin_prims, paper_override, view_bounds) = parse_binary_w2d(&w2d_bytes);
                                            sheet_primitives = bin_prims;
                                            if sheet_paper_size.is_none() && paper_override.is_some() {
                                                sheet_paper_size = paper_override;
                                            }
                                            sheet_view_bounds = view_bounds;
                                        }
                                    }
                                }
                            }
                        }

                        eprintln!("DWF Sheet '{}': {} primitives, fallback_img={}", section_title, sheet_primitives.len(), fallback_img.is_some());
                        if !sheet_primitives.is_empty() || fallback_img.is_some() {
                            sheets.push(DwfSheet {
                                title: section_title,
                                primitives: sheet_primitives,
                                fallback_image: fallback_img,
                                paper_size_pt: sheet_paper_size,
                                is_inch,
                                view_bounds: sheet_view_bounds,
                                transform: sheet_transform,
                            });
                        }
                    }
                }
            }
        }

        // Fallback: If no sheets from manifest, scan archive for any *.w2d or *.jpg/png
        if sheets.is_empty() {
            for i in 0..archive.len() {
                if let Ok(mut f) = archive.by_index(i) {
                    let name = f.name().to_string();
                    if name.ends_with(".w2d") {
                        let mut w2d_bytes = Vec::new();
                        if f.read_to_end(&mut w2d_bytes).is_ok() {
                            let (prims, paper_override, view_bounds) = parse_binary_w2d(&w2d_bytes);
                            if !prims.is_empty() {
                                sheets.push(DwfSheet {
                                    title: format!("Sheet {}", sheets.len() + 1),
                                    primitives: prims,
                                    fallback_image: None,
                                    paper_size_pt: paper_override,
                                    is_inch: false,
                                    view_bounds,
                                    transform: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Secondary fallback: Any raster image
        if sheets.is_empty() {
            for i in 0..archive.len() {
                if let Ok(mut f) = archive.by_index(i) {
                    let name = f.name().to_string();
                    let lower = name.to_lowercase();
                    if lower.ends_with(".jpg") || lower.ends_with(".png") || lower.ends_with(".jpeg") {
                        let mut bytes = Vec::new();
                        if f.read_to_end(&mut bytes).is_ok() && bytes.len() > 1024 {
                            let (norm_bytes, norm_ext) = normalize_image_for_pdf(&bytes);
                            sheets.push(DwfSheet {
                                title: format!("Sheet {}", sheets.len() + 1),
                                primitives: Vec::new(),
                                fallback_image: Some((norm_bytes, norm_ext)),
                                paper_size_pt: None,
                                is_inch: false,
                                view_bounds: None,
                                transform: None,
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

        build_dwf_pdf(&sheets, options, title_from_manifest)
    } else {
        // 2. Uncompressed legacy ASCII DWF (DWF V00.xx)
        let content_str = String::from_utf8_lossy(dwf_bytes);
        let primitives = parse_legacy_ascii_dwf(&content_str);
        if primitives.is_empty() {
            return Err(ConversionError::EmptyDrawing);
        }
        let sheet = DwfSheet {
            title: "Legacy DWF".to_string(),
            primitives,
            fallback_image: None,
            paper_size_pt: Some((841.89, 595.28)), // Standard A4 landscape
            is_inch: false,
            view_bounds: None,
            transform: None,
        };
        build_dwf_pdf(&[sheet], options, Some("Legacy DWF".to_string()))
    }
}

/// Builds the final PDF document from parsed DWF sheets.
fn build_dwf_pdf(
    sheets: &[DwfSheet],
    options: &ConversionOptions,
    doc_title_opt: Option<String>,
) -> ConversionResult<Vec<u8>> {
    let mut pdf_doc = Document::with_version("1.7");
    let mut page_ids = Vec::new();

    for sheet in sheets {
        let (page_width, page_height) = sheet.paper_size_pt.unwrap_or((841.89, 595.28));
        let mut image_manager = ImageManager::new();
        let mut resources = Dictionary::new();
        let mut content_ops = Vec::new();

        // 1. Draw Page Background (white)
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

        // 2. Draw fallback image (if 3D / raster DWF)
        if sheet.primitives.is_empty() {
            if let Some((img_bytes, ext)) = &sheet.fallback_image {
                if let Ok(img) = Image::from_bytes(img_bytes.clone(), Some(format!("dwf_img.{}", ext))) {
                    let (img_w, img_h) = img.dimensions();
                    if let Ok(image_id) = image_manager.embed_image(&mut pdf_doc, img) {
                        let margin = if sheet.paper_size_pt.is_some() { 0.0 } else { options.margin_pt.max(10.0) };
                        let target_w = page_width - 2.0 * margin;
                        let target_h = page_height - 2.0 * margin;
                        let scale = (target_w / img_w as f64).min(target_h / img_h as f64);
                        let draw_w = img_w as f64 * scale;
                        let draw_h = img_h as f64 * scale;
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
                }
            }
        } else {
            let margin = if sheet.paper_size_pt.is_some() { 0.0 } else { options.margin_pt.max(10.0) };
            let target_w = page_width - 2.0 * margin;
            let target_h = page_height - 2.0 * margin;

            let factor = if sheet.is_inch {
                72.0
            } else {
                72.0 / 25.4
            };

            let (min_x, min_y, max_x, max_y) = if let Some(vb) = sheet.view_bounds {
                vb
            } else {
                let mut min_x = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                for prim in &sheet.primitives {
                    match prim {
                        DwfPrimitive::Line { p1, p2, .. } => {
                            min_x = min_x.min(p1.0).min(p2.0);
                            max_x = max_x.max(p1.0).max(p2.0);
                            min_y = min_y.min(p1.1).min(p2.1);
                            max_y = max_y.max(p1.1).max(p2.1);
                        }
                        DwfPrimitive::Polyline { pts, .. } => {
                            for pt in pts {
                                min_x = min_x.min(pt.0);
                                max_x = max_x.max(pt.0);
                                min_y = min_y.min(pt.1);
                                max_y = max_y.max(pt.1);
                            }
                        }
                        DwfPrimitive::Text { pos, .. } => {
                            min_x = min_x.min(pos.0);
                            max_x = max_x.max(pos.0);
                            min_y = min_y.min(pos.1);
                            max_y = max_y.max(pos.1);
                        }
                        _ => {}
                    }
                }
                (min_x, min_y, max_x, max_y)
            };

            let geom_w = if max_x > min_x { max_x - min_x } else { 1.0 };
            let geom_h = if max_y > min_y { max_y - min_y } else { 1.0 };
            let auto_scale = (target_w / geom_w).min(target_h / geom_h);

            let (map_coord, scale): (Box<dyn Fn((f64, f64)) -> (f64, f64)>, f64) = if let Some(t) = sheet.transform {
                (Box::new(move |p: (f64, f64)| -> (f64, f64) {
                    let x_u = p.0 * t[0] + p.1 * t[4] + t[12];
                    let y_u = p.0 * t[1] + p.1 * t[5] + t[13];
                    (x_u * factor, y_u * factor)
                }), (t[0].abs() + t[5].abs()) / 2.0)
            } else {
                let draw_w = geom_w * auto_scale;
                let draw_h = geom_h * auto_scale;
                let off_x = margin + (target_w - draw_w) / 2.0 - min_x * auto_scale;
                let off_y = margin + (target_h - draw_h) / 2.0 - min_y * auto_scale;

                (Box::new(move |p: (f64, f64)| -> (f64, f64) {
                    (p.0 * auto_scale + off_x, p.1 * auto_scale + off_y)
                }), auto_scale)
            };

            // Embed font for text primitives if any
            let has_text = sheet.primitives.iter().any(|p| matches!(p, DwfPrimitive::Text { .. }));
            if has_text {
                let mut font_dict = Dictionary::new();
                let mut helvetica = Dictionary::new();
                helvetica.set("Type", Object::Name(b"Font".to_vec()));
                helvetica.set("Subtype", Object::Name(b"Type1".to_vec()));
                helvetica.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
                helvetica.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
                font_dict.set("F1", Object::Dictionary(helvetica));
                resources.set("Font", Object::Dictionary(font_dict));
            }

            // Draw embedded images at their exact logical coordinates
            for prim in &sheet.primitives {
                if let DwfPrimitive::Image { bounds, bytes, ext } = prim {
                    if let Ok(img) = Image::from_bytes(bytes.clone(), Some(format!("dwf_embedded.{}", ext))) {
                        if let Ok(image_id) = image_manager.embed_image(&mut pdf_doc, img) {
                            let res_name = image_manager.add_to_resources(&mut resources, image_id);
                            let (ix, iy, iw, ih) = *bounds;
                            let (draw_x, draw_y, draw_w, draw_h) = if iw > 0.0 && ih > 0.0 {
                                let p1 = map_coord((ix, iy));
                                let p2 = map_coord((ix + iw, iy + ih));
                                let dx = p1.0.min(p2.0);
                                let dy = p1.1.min(p2.1);
                                let dw = (p2.0 - p1.0).abs();
                                let dh = (p2.1 - p1.1).abs();
                                (dx, dy, dw, dh)
                            } else {
                                (margin, margin, target_w, target_h)
                            };

                            let draw_ops = ImageManager::draw_image(
                                &res_name,
                                draw_x as f32,
                                draw_y as f32,
                                draw_w as f32,
                                draw_h as f32,
                            );
                            content_ops.extend(draw_ops);
                        }
                    }
                }
            }

            // Draw vector paths
            content_ops.push(Operation::new("q", vec![]));

            for prim in &sheet.primitives {
                match prim {
                    DwfPrimitive::Line { p1, p2, color, weight } => {
                        let (x1, y1) = map_coord(*p1);
                        let (x2, y2) = map_coord(*p2);

                        if !x1.is_finite() || !y1.is_finite() || !x2.is_finite() || !y2.is_finite() {
                            continue;
                        }
                        if x1 < -5000.0 || x1 > 20000.0 || y1 < -5000.0 || y1 > 20000.0 {
                            continue;
                        }

                        let r = color.0 as f32 / 255.0;
                        let g = color.1 as f32 / 255.0;
                        let b = color.2 as f32 / 255.0;
                        // DWF lineweights are part of the plotted output, so they are
                        // authoritative here; they are not the optional CAD lineweight display.
                        let stroke_w = *weight;

                        content_ops.push(Operation::new("RG", vec![Object::Real(r), Object::Real(g), Object::Real(b)]));
                        content_ops.push(Operation::new("w", vec![Object::Real(stroke_w)]));
                        content_ops.push(Operation::new("m", vec![Object::Real(x1 as f32), Object::Real(y1 as f32)]));
                        content_ops.push(Operation::new("l", vec![Object::Real(x2 as f32), Object::Real(y2 as f32)]));
                        content_ops.push(Operation::new("S", vec![]));
                    }
                    DwfPrimitive::Polyline { pts, color, fill_color, weight } => {
                        if pts.len() < 2 {
                            continue;
                        }
                        let mapped: Vec<(f64, f64)> = pts.iter().map(|&p| map_coord(p)).collect();
                        if mapped.iter().any(|p| !p.0.is_finite() || !p.1.is_finite() || p.0 < -5000.0 || p.0 > 20000.0 || p.1 < -5000.0 || p.1 > 20000.0) {
                            continue;
                        }

                        let r = color.0 as f32 / 255.0;
                        let g = color.1 as f32 / 255.0;
                        let b = color.2 as f32 / 255.0;
                        // See above: DWF weights are plotted output, always honor them.
                        let stroke_w = *weight;
                        content_ops.push(Operation::new("RG", vec![Object::Real(r), Object::Real(g), Object::Real(b)]));
                        content_ops.push(Operation::new("w", vec![Object::Real(stroke_w)]));

                        if let Some(fc) = fill_color {
                            let fr = fc.0 as f32 / 255.0;
                            let fg = fc.1 as f32 / 255.0;
                            let fb = fc.2 as f32 / 255.0;
                            content_ops.push(Operation::new("rg", vec![Object::Real(fr), Object::Real(fg), Object::Real(fb)]));
                        }

                        let p0 = mapped[0];
                        content_ops.push(Operation::new("m", vec![Object::Real(p0.0 as f32), Object::Real(p0.1 as f32)]));

                        for pt in &mapped[1..] {
                            content_ops.push(Operation::new("l", vec![Object::Real(pt.0 as f32), Object::Real(pt.1 as f32)]));
                        }

                        if fill_color.is_some() {
                            content_ops.push(Operation::new("B", vec![]));
                        } else {
                            content_ops.push(Operation::new("S", vec![]));
                        }
                    }
                    DwfPrimitive::Text { pos, size, color, content } => {
                        let (text_x, text_y) = map_coord(*pos);
                        if !text_x.is_finite() || !text_y.is_finite() || text_x < -5000.0 || text_x > 20000.0 || text_y < -5000.0 || text_y > 20000.0 {
                            continue;
                        }
                        let r = color.0 as f32 / 255.0;
                        let g = color.1 as f32 / 255.0;
                        let b = color.2 as f32 / 255.0;
                        let font_sz = (*size * scale as f32).max(1.0);

                        let raw_lines: Vec<&str> = content.split('\n').collect();
                        let mut lines: Vec<String> = Vec::new();
                        for rl in raw_lines {
                            let trimmed = rl.trim();
                            if trimmed.len() <= 60 {
                                lines.push(trimmed.to_string());
                            } else {
                                let words: Vec<&str> = trimmed.split_whitespace().collect();
                                let mut cur_line = String::new();
                                for w in words {
                                    if cur_line.is_empty() {
                                        cur_line.push_str(w);
                                    } else if cur_line.len() + 1 + w.len() <= 60 {
                                        cur_line.push(' ');
                                        cur_line.push_str(w);
                                    } else {
                                        lines.push(cur_line);
                                        cur_line = w.to_string();
                                    }
                                }
                                if !cur_line.is_empty() {
                                    lines.push(cur_line);
                                }
                            }
                        }

                        let line_spacing = font_sz * 1.25;

                        for (line_idx, line) in lines.iter().enumerate() {
                            let cur_ty = text_y - line_idx as f64 * line_spacing as f64;
                            let mut winansi_bytes = Vec::with_capacity(line.len());
                            for ch in line.chars() {
                                if let Some(byte) = crate::pdf::builder::char_to_winansi(ch) {
                                    winansi_bytes.push(byte);
                                } else if ch.is_ascii() {
                                    winansi_bytes.push(ch as u8);
                                } else {
                                    winansi_bytes.push(b'?');
                                }
                            }

                            content_ops.push(Operation::new("q", vec![]));
                            content_ops.push(Operation::new("rg", vec![Object::Real(r), Object::Real(g), Object::Real(b)]));
                            content_ops.push(Operation::new("BT", vec![]));
                            content_ops.push(Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), Object::Real(font_sz)]));
                            content_ops.push(Operation::new(
                                "Tm",
                                vec![
                                    Object::Real(1.0), Object::Real(0.0),
                                    Object::Real(0.0), Object::Real(1.0),
                                    Object::Real(text_x as f32), Object::Real(cur_ty as f32),
                                ],
                            ));
                            content_ops.push(Operation::new("Tj", vec![Object::string_literal(winansi_bytes)]));
                            content_ops.push(Operation::new("ET", vec![]));
                            content_ops.push(Operation::new("Q", vec![]));
                        }
                    }
                    _ => {}
                }
            }

            content_ops.push(Operation::new("Q", vec![]));
        }

        // Add page to doc
        let content_id = pdf_doc.add_object(Stream::new(Dictionary::new(), Content { operations: content_ops }.encode().map_err(|e| {
            ConversionError::PdfGeneration(format!("Failed to encode DWF page content: {}", e))
        })?));

        let page_dict = Dictionary::from_iter(vec![
            ("Type", "Page".into()),
            ("Parent", Object::Null), // Will be updated by Pages dict
            (
                "MediaBox",
                Object::Array(vec![
                    0.into(),
                    0.into(),
                    (page_width as f32).into(),
                    (page_height as f32).into(),
                ]),
            ),
            ("Contents", Object::Reference(content_id)),
            ("Resources", Object::Dictionary(resources)),
        ]);

        let page_id = pdf_doc.add_object(page_dict);
        page_ids.push(page_id);
    }

    // Create Pages object
    let pages_dict = Dictionary::from_iter(vec![
        ("Type", "Pages".into()),
        ("Count", (page_ids.len() as i64).into()),
        ("Kids", Object::Array(page_ids.iter().map(|&id| Object::Reference(id)).collect())),
    ]);
    let pages_id = pdf_doc.add_object(pages_dict);

    // Update Parent reference in each page
    for &page_id in &page_ids {
        if let Ok(page_obj) = pdf_doc.get_object_mut(page_id) {
            if let Object::Dictionary(ref mut dict) = page_obj {
                dict.set("Parent", Object::Reference(pages_id));
            }
        }
    }

    // Set Root / Catalog
    let mut catalog = Dictionary::from_iter(vec![
        ("Type", "Catalog".into()),
        ("Pages", Object::Reference(pages_id)),
    ]);

    // Metadata
    let doc_title = options.metadata.title.clone().or(doc_title_opt).unwrap_or_else(|| "DWF Drawing".to_string());
    let mut info = Dictionary::from_iter(vec![
        ("Title", Object::string_literal(doc_title.as_str())),
        ("Producer", Object::string_literal("dwgpdf (hipdf)")),
    ]);
    if let Some(author) = &options.metadata.author {
        info.set("Author", Object::string_literal(author.as_str()));
    }
    if let Some(subject) = &options.metadata.subject {
        info.set("Subject", Object::string_literal(subject.as_str()));
    }
    if let Some(creator) = &options.metadata.creator {
        info.set("Creator", Object::string_literal(creator.as_str()));
    }

    catalog.set("Metadata", Object::Null);
    let catalog_id = pdf_doc.add_object(catalog);
    let info_id = pdf_doc.add_object(info);

    pdf_doc.trailer.set("Root", Object::Reference(catalog_id));
    pdf_doc.trailer.set("Info", Object::Reference(info_id));

    let mut output = Vec::new();
    pdf_doc.save_to(&mut output).map_err(|e| {
        ConversionError::PdfGeneration(format!("Failed to serialize PDF: {}", e))
    })?;

    Ok(output)
}
