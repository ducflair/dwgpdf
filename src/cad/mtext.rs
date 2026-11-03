/// Decodes standard AutoCAD text escape sequences (%%c, %%d, %%p, etc.)
pub fn decode_cad_text_escapes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            if chars.peek() == Some(&'%') {
                chars.next(); // consume second '%'
                match chars.next() {
                    Some('c') | Some('C') => result.push('Ø'),
                    Some('d') | Some('D') => result.push('°'),
                    Some('p') | Some('P') => result.push('±'),
                    Some('%') => result.push('%'),
                    Some('u') | Some('U') => {} // underline toggle
                    Some('o') | Some('O') => {} // overline toggle
                    Some(other) => {
                        result.push('%');
                        result.push('%');
                        result.push(other);
                    }
                    None => {
                        result.push('%');
                        result.push('%');
                    }
                }
            } else {
                result.push(ch);
            }
        } else if ch == '\\' {
            // Check for Unicode escape \U+XXXX
            let remaining: String = chars.clone().take(6).collect();
            if remaining.len() >= 6 && (remaining.starts_with("U+") || remaining.starts_with("u+")) {
                let hex_str = &remaining[2..6];
                if let Ok(code) = u32::from_str_radix(hex_str, 16) {
                    if let Some(unicode_char) = char::from_u32(code) {
                        result.push(unicode_char);
                        for _ in 0..6 {
                            chars.next();
                        }
                        continue;
                    }
                }
            }
            result.push(ch);
        } else {
            result.push(ch);
        }
    }

    result
}

/// Parses an initial or prominent `\H...x;` scale factor from MTEXT value.
pub fn parse_mtext_height_scale(text: &str) -> f64 {
    if let Some(pos) = text.find("\\H") {
        let rest = &text[pos + 2..];
        if let Some(semi) = rest.find(';') {
            let num_str = &rest[..semi];
            if let Some(scale_str) = num_str.strip_suffix('x').or_else(|| num_str.strip_suffix('X')) {
                if let Ok(scale) = scale_str.parse::<f64>() {
                    if scale > 0.01 && scale < 100.0 {
                        return scale;
                    }
                }
            }
        }
    }
    1.0
}

/// Official Adobe Font Metrics (AFM) character widths for standard Helvetica / Arial normalized to font_size = 1.0.
pub fn helvetica_char_width(c: char) -> f64 {
    match c {
        ' ' | '\u{00A0}' => 0.278,
        '!' => 0.278,
        '"' => 0.355,
        '#' => 0.556,
        '$' => 0.556,
        '%' => 0.889,
        '&' => 0.667,
        '\'' => 0.222,
        '(' | ')' => 0.333,
        '*' => 0.389,
        '+' => 0.584,
        ',' | '.' => 0.278,
        '-' | '–' | '—' => 0.333,
        '/' => 0.278,
        '0'..='9' => 0.556,
        ':' | ';' => 0.278,
        '<' | '=' | '>' => 0.584,
        '?' => 0.556,
        '@' => 1.015,
        'A' => 0.667,
        'B' => 0.667,
        'C' => 0.722,
        'D' => 0.722,
        'E' => 0.667,
        'F' => 0.611,
        'G' => 0.778,
        'H' => 0.722,
        'I' => 0.278,
        'J' => 0.500,
        'K' => 0.667,
        'L' => 0.556,
        'M' => 0.833,
        'N' => 0.722,
        'O' => 0.778,
        'P' => 0.667,
        'Q' => 0.778,
        'R' => 0.722,
        'S' => 0.667,
        'T' => 0.611,
        'U' => 0.722,
        'V' => 0.667,
        'W' => 0.944,
        'X' => 0.667,
        'Y' => 0.667,
        'Z' => 0.611,
        '[' | ']' => 0.278,
        '\\' => 0.278,
        '^' => 0.469,
        '_' => 0.556,
        '`' => 0.222,
        'a' => 0.556,
        'b' => 0.556,
        'c' => 0.500,
        'd' => 0.556,
        'e' => 0.556,
        'f' => 0.278,
        'g' => 0.556,
        'h' => 0.556,
        'i' => 0.222,
        'j' => 0.222,
        'k' => 0.500,
        'l' => 0.222,
        'm' => 0.833,
        'n' => 0.556,
        'o' => 0.556,
        'p' => 0.556,
        'q' => 0.556,
        'r' => 0.333,
        's' => 0.500,
        't' => 0.278,
        'u' => 0.556,
        'v' => 0.500,
        'w' => 0.722,
        'x' => 0.500,
        'y' => 0.500,
        'z' => 0.500,
        '{' | '|' | '}' => 0.334,
        '~' => 0.584,
        _ => 0.556,
    }
}

/// Estimates character width relative to font height for standard proportional sans-serif (Arial/Helvetica).
pub fn estimate_char_width(c: char, font_height: f64) -> f64 {
    helvetica_char_width(c) * font_height
}

pub fn estimate_str_width(s: &str, font_height: f64) -> f64 {
    s.chars().map(|c| estimate_char_width(c, font_height)).sum()
}

/// Paragraph alignment options parsed from MTEXT formatting codes (\p...; or \px...;).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MTextAlignment {
    #[default]
    Default,
    Left,
    Center,
    Right,
    Justified,
    Distributed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MTextLineInfo {
    pub text: String,
    pub indent: f64,
    pub width_factor: f64,
    pub tracking_factor: f64,
    pub is_bold: bool,
    pub is_italic: bool,
    pub height_scale: f64,
    pub alignment: MTextAlignment,
}

/// Parses paragraph properties from `\p...;` or `\px...;` formatting blocks.
pub fn parse_paragraph_formatting(param: &str) -> (Option<MTextAlignment>, Option<f64>) {
    let mut alignment = None;
    let mut indent = None;

    let clean = param.trim();
    for token in clean.split(',') {
        let token = token.trim();
        let sub = token.strip_prefix('x').unwrap_or(token);

        if let Some(rest) = sub.strip_prefix('q') {
            match rest.chars().next().map(|c| c.to_ascii_lowercase()) {
                Some('l') => alignment = Some(MTextAlignment::Left),
                Some('c') => alignment = Some(MTextAlignment::Center),
                Some('r') => alignment = Some(MTextAlignment::Right),
                Some('j') | Some('*') => alignment = Some(MTextAlignment::Justified),
                Some('d') => alignment = Some(MTextAlignment::Distributed),
                _ => {}
            }
        } else if let Some(rest) = sub.strip_prefix('i') {
            if let Ok(val) = rest.parse::<f64>() {
                if val > 0.0 {
                    indent = Some(val);
                }
            }
        }
    }

    (alignment, indent)
}

#[derive(Debug, Clone, PartialEq)]
pub struct MTextInfo {
    pub lines: Vec<MTextLineInfo>,
    pub is_bold: bool,
    pub is_italic: bool,
    pub width_factor: f64,
    pub tracking_factor: f64,
}

/// Parses and cleans MTEXT formatted strings, extracting plain text lines.
pub fn clean_mtext(text: &str) -> Vec<String> {
    clean_mtext_with_wrap(text, 0.0, 1.0)
}

/// Parses and cleans MTEXT formatted strings, wrapping lines if rectangle_width > 0.
pub fn clean_mtext_with_wrap(text: &str, rectangle_width: f64, font_height: f64) -> Vec<String> {
    let details = parse_mtext_details(text, rectangle_width, font_height);
    details.lines.into_iter().map(|l| l.text).collect()
}

/// Rich MTEXT parser extracting formatting flags (bold, italic), character scaling,
/// paragraph indentations, and text lines.
pub fn parse_mtext_details(text: &str, rectangle_width: f64, font_height: f64) -> MTextInfo {
    let global_bold = text.contains("|b1") || text.contains("|B1") || text.contains("Arial Bold") || text.contains("Helvetica-Bold");
    let global_italic = text.contains("|i1") || text.contains("|I1") || text.contains("Italic") || text.contains("italic") || text.contains("Oblique") || text.contains("oblique");

    let normalized = text
        .replace("\r\n", "\\P")
        .replace('\n', "\\P")
        .replace('\r', "\\P")
        .replace("^J", "\\P")
        .replace('\u{2028}', "\\P")
        .replace('\u{2029}', "\\P")
        .replace("â€¨", "\\P");

    let mut raw_lines: Vec<MTextLineInfo> = Vec::new();
    let mut current_line = String::new();
    let mut current_indent = 0.0;
    let mut current_wf = 1.0;
    let mut current_tracking = 1.0;
    let mut current_bold = global_bold;
    let mut current_italic = global_italic;
    let mut current_height_scale = 1.0;
    let mut current_alignment = MTextAlignment::Default;
    let mut chars = normalized.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '{' | '}' => {
                // Formatting group scopes - skip
            }
            '\\' => {
                match chars.next() {
                    Some('\\') => current_line.push('\\'),
                    Some('{') => current_line.push('{'),
                    Some('}') => current_line.push('}'),
                    Some('~') => current_line.push(' '), // Non-breaking space
                    Some('P') => {
                        // Paragraph / Newline break (uppercase P only!)
                        raw_lines.push(MTextLineInfo {
                            text: decode_cad_text_escapes(&current_line),
                            indent: current_indent,
                            width_factor: current_wf,
                            tracking_factor: current_tracking,
                            is_bold: current_bold,
                            is_italic: current_italic,
                            height_scale: current_height_scale,
                            alignment: current_alignment,
                        });
                        current_line.clear();
                        current_indent = 0.0;
                    }
                    Some('L') | Some('l') | Some('O') | Some('o') | Some('K') | Some('k') | Some('X') => {
                        // Single-letter format toggle
                    }
                    Some('S') | Some('s') => {
                        // Stacking fraction: \S3#8; -> 3/8
                        let mut frac = String::new();
                        for next_ch in chars.by_ref() {
                            if next_ch == ';' {
                                break;
                            }
                            frac.push(next_ch);
                        }
                        let converted = frac.replace('#', "/").replace('^', "/");
                        current_line.push_str(&converted);
                    }
                    Some('p') => {
                        // Paragraph formatting \p...; or \px...;
                        let mut param = String::new();
                        for next_ch in chars.by_ref() {
                            if next_ch == ';' {
                                break;
                            }
                            param.push(next_ch);
                        }
                        let (parsed_align, parsed_indent) = parse_paragraph_formatting(&param);
                        if let Some(align) = parsed_align {
                            current_alignment = align;
                        }
                        if let Some(ind) = parsed_indent {
                            current_indent = ind;
                        }
                    }
                    Some('T') | Some('t') => {
                        let mut param = String::new();
                        for next_ch in chars.by_ref() {
                            if next_ch == ';' {
                                break;
                            }
                            param.push(next_ch);
                        }
                        if let Ok(val) = param.trim().parse::<f64>() {
                            if val > 0.05 && val < 10.0 {
                                current_tracking = val;
                            }
                        }
                    }
                    Some('W') | Some('w') => {
                        let mut param = String::new();
                        for next_ch in chars.by_ref() {
                            if next_ch == ';' {
                                break;
                            }
                            param.push(next_ch);
                        }
                        if let Ok(val) = param.trim().parse::<f64>() {
                            if val > 0.05 && val < 10.0 {
                                current_wf = val;
                            }
                        }
                    }
                    Some('f') | Some('F') => {
                        let mut param = String::new();
                        for next_ch in chars.by_ref() {
                            if next_ch == ';' {
                                break;
                            }
                            param.push(next_ch);
                        }
                        if param.contains("|b1") || param.contains("|B1") || param.contains("Bold") || param.contains("bold") {
                            current_bold = true;
                        } else if param.contains("|b0") || param.contains("|B0") {
                            current_bold = false;
                        }
                        if param.contains("|i1") || param.contains("|I1") || param.contains("Italic") || param.contains("italic") || param.contains("Oblique") {
                            current_italic = true;
                        } else if param.contains("|i0") || param.contains("|I0") {
                            current_italic = false;
                        }
                    }
                    Some('H') | Some('h') => {
                        let mut param = String::new();
                        for next_ch in chars.by_ref() {
                            if next_ch == ';' {
                                break;
                            }
                            param.push(next_ch);
                        }
                        if let Some(s_str) = param.strip_suffix('x').or_else(|| param.strip_suffix('X')) {
                            if let Ok(scale) = s_str.trim().parse::<f64>() {
                                if scale > 0.01 && scale < 20.0 {
                                    current_height_scale = (current_height_scale * scale).clamp(0.01, 20.0);
                                }
                            }
                        } else if font_height > 0.001 {
                            if let Ok(abs_h) = param.trim().parse::<f64>() {
                                if abs_h > 0.001 {
                                    current_height_scale = abs_h / font_height;
                                }
                            }
                        }
                    }
                    Some(code) if code == 'A' || code == 'a' || code == 'C' || code == 'c' ||
                                  code == 'Q' || code == 'q' => {
                        // Long format code terminated by ';'
                        for next_ch in chars.by_ref() {
                            if next_ch == ';' {
                                break;
                            }
                        }
                    }
                    Some('U') | Some('u') => {
                        if chars.peek() == Some(&'+') {
                            chars.next(); // consume '+'
                            let mut hex_str = String::with_capacity(4);
                            for _ in 0..4 {
                                if let Some(&next_ch) = chars.peek() {
                                    if next_ch.is_ascii_hexdigit() {
                                        hex_str.push(chars.next().unwrap());
                                    } else {
                                        break;
                                    }
                                }
                            }
                            if let Ok(code) = u32::from_str_radix(&hex_str, 16) {
                                if let Some(ch) = char::from_u32(code) {
                                    current_line.push(ch);
                                }
                            }
                        }
                    }
                    Some(_) => {}
                    None => {}
                }
            }
            _ => {
                current_line.push(ch);
            }
        }
    }

    if !current_line.is_empty() || raw_lines.is_empty() {
        raw_lines.push(MTextLineInfo {
            text: decode_cad_text_escapes(&current_line),
            indent: current_indent,
            width_factor: current_wf,
            tracking_factor: current_tracking,
            is_bold: current_bold,
            is_italic: current_italic,
            height_scale: current_height_scale,
            alignment: current_alignment,
        });
    }

    // Now perform word wrapping if rectangle_width is positive and font_height is positive
    let mut lines: Vec<MTextLineInfo> = Vec::new();
    let mut last_was_empty = false;

    for raw in raw_lines {
        let trimmed = raw.text.trim();
        if trimmed.is_empty() {
            if !last_was_empty {
                lines.push(MTextLineInfo {
                    text: String::new(),
                    indent: 0.0,
                    width_factor: raw.width_factor,
                    tracking_factor: raw.tracking_factor,
                    is_bold: raw.is_bold,
                    is_italic: raw.is_italic,
                    height_scale: raw.height_scale,
                    alignment: raw.alignment,
                });
                last_was_empty = true;
            }
            continue;
        }
        last_was_empty = false;

        let eff_font_h = font_height * raw.height_scale;
        let char_extra = if (raw.tracking_factor - 1.0).abs() > 0.01 {
            (raw.tracking_factor - 1.0) * eff_font_h * 0.55
        } else {
            0.0
        };
        let sp_w = estimate_char_width(' ', eff_font_h) * raw.width_factor + char_extra;

        let available_w = (rectangle_width - raw.indent).max(eff_font_h);
        let num_c = trimmed.chars().count();
        let str_w = estimate_str_width(trimmed, eff_font_h) * raw.width_factor
            + char_extra * (num_c.saturating_sub(1) as f64);

        // Only wrap if rectangle_width is larger than a single character (otherwise it's an unset/auto width)
        if rectangle_width > eff_font_h * 0.5 && str_w > available_w {
            let mut cur_line = String::new();
            let mut cur_w = 0.0;
            let mut is_first_subline = true;

            for word in trimmed.split(' ') {
                if word.is_empty() {
                    continue;
                }
                let w_chars = word.chars().count();
                let ww = estimate_str_width(word, eff_font_h) * raw.width_factor
                    + char_extra * (w_chars.saturating_sub(1) as f64);
                let target_w = if is_first_subline { available_w } else { rectangle_width };

                if !cur_line.is_empty() && (cur_w + sp_w + ww > target_w) {
                    lines.push(MTextLineInfo {
                        text: cur_line,
                        indent: if is_first_subline { raw.indent } else { 0.0 },
                        width_factor: raw.width_factor,
                        tracking_factor: raw.tracking_factor,
                        is_bold: raw.is_bold,
                        is_italic: raw.is_italic,
                        height_scale: raw.height_scale,
                        alignment: raw.alignment,
                    });
                    cur_line = word.to_string();
                    cur_w = ww;
                    is_first_subline = false;
                } else {
                    if !cur_line.is_empty() {
                        cur_line.push(' ');
                        cur_line.push_str(word);
                        cur_w += sp_w + ww;
                    } else {
                        cur_line.push_str(word);
                        cur_w = ww;
                    }
                }
            }
            if !cur_line.is_empty() {
                lines.push(MTextLineInfo {
                    text: cur_line,
                    indent: if is_first_subline { raw.indent } else { 0.0 },
                    width_factor: raw.width_factor,
                    tracking_factor: raw.tracking_factor,
                    is_bold: raw.is_bold,
                    is_italic: raw.is_italic,
                    height_scale: raw.height_scale,
                    alignment: raw.alignment,
                });
            }
        } else {
            lines.push(MTextLineInfo {
                text: trimmed.to_string(),
                indent: raw.indent,
                width_factor: raw.width_factor,
                tracking_factor: raw.tracking_factor,
                is_bold: raw.is_bold,
                is_italic: raw.is_italic,
                height_scale: raw.height_scale,
                alignment: raw.alignment,
            });
        }
    }

    let global_wf = lines.first().map(|l| l.width_factor).unwrap_or(1.0);
    let global_tracking = lines.first().map(|l| l.tracking_factor).unwrap_or(1.0);

    MTextInfo {
        lines,
        is_bold: global_bold,
        is_italic: global_italic,
        width_factor: global_wf,
        tracking_factor: global_tracking,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_cad_escapes() {
        assert_eq!(decode_cad_text_escapes("Diameter %%c50"), "Diameter Ø50");
        assert_eq!(decode_cad_text_escapes("Angle 45%%d"), "Angle 45°");
        assert_eq!(decode_cad_text_escapes("Tolerance %%p0.05"), "Tolerance ±0.05");
    }

    #[test]
    fn test_clean_mtext() {
        let raw = r"{\fArial|b0|i0|c0;\C1;Hello \PWorld}";
        let lines = clean_mtext(raw);
        assert_eq!(lines, vec!["Hello", "World"]);
    }

    #[test]
    fn test_note_wrapping() {
        let raw = r"{\fArial|b0|i0|c0|p34;\H0.5x;1. WORK SHALL CONFORM TO ALL APPLICABLE CODES AND ORDINANCES, INCLUDING BUT NOT LIMITED TO: BUILDING, PLUMBING, ELECTRICAL, AND MECHANICAL CODES. \P\fLucida Grande|b0|i0|c0|p34;â€¨\P\fArial|b0|i0|c0|p34;2. ANY ERRORS}";
        let lines = clean_mtext_with_wrap(raw, 730.0, 36.0);
        assert_eq!(lines[0], "1. WORK SHALL CONFORM TO ALL APPLICABLE CODES AND ORDINANCES,");
        assert_eq!(lines[1], "INCLUDING BUT NOT LIMITED TO: BUILDING, PLUMBING, ELECTRICAL, AND");
        assert_eq!(lines[2], "MECHANICAL CODES.");
        assert_eq!(lines[3], "");
        assert_eq!(lines[4], "2. ANY ERRORS");
    }

    #[test]
    fn test_parse_mtext_details() {
        let raw = r"\pi48.38;{\fArial|b1|i0;OPEN-SOURCE \P\pi28.61;ARCHITECTURAL\P\pi55.90;\T0.75;DRAWING SET}";
        let info = parse_mtext_details(raw, 0.0, 12.0);
        assert!(info.is_bold);
        assert!(!info.is_italic);
        assert_eq!(info.lines.len(), 3);
        assert_eq!(info.lines[0].text, "OPEN-SOURCE");
        assert!((info.lines[0].indent - 48.38).abs() < 1e-4);
        assert_eq!(info.lines[1].text, "ARCHITECTURAL");
        assert!((info.lines[1].indent - 28.61).abs() < 1e-4);
        assert_eq!(info.lines[2].text, "DRAWING SET");
        assert!((info.lines[2].indent - 55.90).abs() < 1e-4);
        assert!((info.lines[2].tracking_factor - 0.75).abs() < 1e-4);
    }
}
