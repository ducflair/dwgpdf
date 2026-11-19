/// Decodes standard AutoCAD text escape sequences (%%c, %%d, %%p, etc.)
pub fn decode_cad_text_escapes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            if chars.peek() == Some(&'%') {
                chars.next(); // consume second '%'
                match chars.peek() {
                    Some('c') | Some('C') => {
                        chars.next();
                        result.push('Ø');
                    }
                    Some('d') | Some('D') => {
                        chars.next();
                        result.push('°');
                    }
                    Some('p') | Some('P') => {
                        chars.next();
                        result.push('±');
                    }
                    Some('%') => {
                        chars.next();
                        result.push('%');
                    }
                    Some('u') | Some('U') => {
                        chars.next(); // underline toggle
                    }
                    Some('o') | Some('O') => {
                        chars.next(); // overline toggle
                    }
                    Some(d) if d.is_ascii_digit() => {
                        let mut num_str = String::new();
                        while let Some(&digit) = chars.peek() {
                            if digit.is_ascii_digit() && num_str.len() < 3 {
                                num_str.push(digit);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        if let Ok(code) = num_str.parse::<u32>() {
                            if let Some(c) = char::from_u32(code) {
                                result.push(c);
                            }
                        }
                    }
                    Some(_) => {
                        let other = chars.next().unwrap();
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
        } else if ch == '^' {
            match chars.peek() {
                Some('I') | Some('i') => {
                    chars.next();
                    result.push_str("    ");
                }
                Some('J') | Some('j') | Some('M') | Some('m') => {
                    chars.next();
                    result.push(' ');
                }
                _ => result.push(ch),
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

/// Official Adobe Font Metrics (AFM) character widths for standard Times-Roman normalized to font_size = 1.0.
pub fn times_char_width(c: char) -> f64 {
    match c {
        ' ' | '\u{00A0}' => 0.250,
        '!' => 0.333,
        '"' => 0.408,
        '#' => 0.500,
        '$' => 0.500,
        '%' => 0.833,
        '&' => 0.778,
        '\'' => 0.333,
        '(' | ')' => 0.333,
        '*' => 0.500,
        '+' => 0.564,
        ',' | '.' => 0.250,
        '-' | '–' | '—' => 0.333,
        '/' => 0.278,
        '0'..='9' => 0.500,
        ':' | ';' => 0.278,
        '<' | '=' | '>' => 0.564,
        '?' => 0.444,
        '@' => 0.921,
        'A' => 0.722,
        'B' => 0.667,
        'C' => 0.667,
        'D' => 0.722,
        'E' => 0.611,
        'F' => 0.556,
        'G' => 0.722,
        'H' => 0.722,
        'I' => 0.333,
        'J' => 0.389,
        'K' => 0.722,
        'L' => 0.611,
        'M' => 0.889,
        'N' => 0.722,
        'O' => 0.722,
        'P' => 0.556,
        'Q' => 0.722,
        'R' => 0.667,
        'S' => 0.556,
        'T' => 0.611,
        'U' => 0.722,
        'V' => 0.722,
        'W' => 0.944,
        'X' => 0.722,
        'Y' => 0.722,
        'Z' => 0.611,
        '[' | ']' => 0.333,
        '\\' => 0.278,
        '^' => 0.469,
        '_' => 0.500,
        '`' => 0.333,
        'a' => 0.444,
        'b' => 0.500,
        'c' => 0.444,
        'd' => 0.500,
        'e' => 0.444,
        'f' => 0.333,
        'g' => 0.500,
        'h' => 0.500,
        'i' => 0.278,
        'j' => 0.278,
        'k' => 0.500,
        'l' => 0.278,
        'm' => 0.778,
        'n' => 0.500,
        'o' => 0.500,
        'p' => 0.500,
        'q' => 0.500,
        'r' => 0.333,
        's' => 0.389,
        't' => 0.278,
        'u' => 0.500,
        'v' => 0.500,
        'w' => 0.722,
        'x' => 0.500,
        'y' => 0.500,
        'z' => 0.444,
        '{' | '|' | '}' => 0.480,
        '~' => 0.541,
        _ => 0.500,
    }
}

/// Standard font categories mapped to the 14 standard PDF Type 1 fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontCategory {
    #[default]
    SansSerif,
    Monospace,
    Serif,
}

impl FontCategory {
    /// Ratio of uppercase cap-height to nominal font size.
    pub fn cap_height_ratio(&self) -> f64 {
        match self {
            FontCategory::SansSerif => 0.718,
            FontCategory::Monospace => 0.600,
            FontCategory::Serif => 0.662,
        }
    }

    /// Ratio of descender depth to nominal font size.
    pub fn descender_ratio(&self) -> f64 {
        match self {
            FontCategory::SansSerif => 0.207,
            FontCategory::Monospace => 0.157,
            FontCategory::Serif => 0.217,
        }
    }
}

pub fn char_width_in_category(c: char, category: FontCategory) -> f64 {
    match category {
        FontCategory::Monospace => 0.600,
        FontCategory::SansSerif => helvetica_char_width(c),
        FontCategory::Serif => times_char_width(c),
    }
}

pub fn estimate_str_width_category(s: &str, font_height: f64, category: FontCategory) -> f64 {
    match category {
        FontCategory::Monospace => (s.chars().count() as f64) * 0.600 * font_height,
        FontCategory::SansSerif => s.chars().map(|c| helvetica_char_width(c) * font_height).sum(),
        FontCategory::Serif => s.chars().map(|c| times_char_width(c) * font_height).sum(),
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
pub struct MTextSpanInfo {
    pub text: String,
    pub is_bold: bool,
    pub is_italic: bool,
    pub height_scale: f64,
    pub width_factor: f64,
    pub tracking_factor: f64,
    pub oblique_angle: f64,
    pub color_aci: Option<u8>,
    pub color_rgb: Option<(u8, u8, u8)>,
    pub is_underline: bool,
    pub is_strikethrough: bool,
    pub is_overline: bool,
    pub font_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MTextLineInfo {
    pub text: String,
    pub spans: Vec<MTextSpanInfo>,
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
    let mut left_margin = 0.0;
    let mut first_line_indent = 0.0;
    let mut has_indent = false;

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
        } else if let Some(rest) = sub.strip_prefix('l') {
            if let Ok(val) = rest.parse::<f64>() {
                left_margin = val;
                has_indent = true;
            }
        } else if let Some(rest) = sub.strip_prefix('i') {
            if let Ok(val) = rest.parse::<f64>() {
                first_line_indent = val;
                has_indent = true;
            }
        }
    }

    if has_indent {
        indent = Some((left_margin + first_line_indent).max(0.0));
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
/// paragraph indentations, and text lines (using standard TrueType / Helvetica metrics).
pub fn parse_mtext_details(text: &str, rectangle_width: f64, font_height: f64) -> MTextInfo {
    parse_mtext_details_with_shx(text, rectangle_width, font_height, false)
}

/// Rich MTEXT parser with support for SHX stroke font wrapping metrics.
/// When `is_shx` is true, word wrapping against `rectangle_width` uses CAD standard SHX font
/// cell aspect ratio (~0.95 * height per char cell), matching AutoCAD's line-breaking engine.
pub fn parse_mtext_details_with_shx(text: &str, rectangle_width: f64, font_height: f64, is_shx: bool) -> MTextInfo {
    let global_bold = text.contains("|b1") || text.contains("|B1") || text.contains("Arial Bold") || text.contains("Helvetica-Bold");
    let global_italic = text.contains("|i1") || text.contains("|I1") || text.contains("Italic") || text.contains("italic") || text.contains("Oblique") || text.contains("oblique");

    let normalized = text
        .replace("\r\n", "\\P")
        .replace('\n', "\\P")
        .replace('\r', "\\P")
        .replace("^J", "\\P")
        .replace("^j", "\\P")
        .replace('\u{2028}', "\\P")
        .replace('\u{2029}', "\\P")
        .replace("â€¨", "\\P");

    #[derive(Debug, Clone)]
    struct FormatState {
        is_bold: bool,
        is_italic: bool,
        height_scale: f64,
        width_factor: f64,
        tracking_factor: f64,
        oblique_angle: f64,
        color_aci: Option<u8>,
        color_rgb: Option<(u8, u8, u8)>,
        is_underline: bool,
        is_strikethrough: bool,
        is_overline: bool,
        font_name: Option<String>,
    }

    let init_state = FormatState {
        is_bold: global_bold,
        is_italic: global_italic,
        height_scale: 1.0,
        width_factor: 1.0,
        tracking_factor: 1.0,
        oblique_angle: 0.0,
        color_aci: None,
        color_rgb: None,
        is_underline: false,
        is_strikethrough: false,
        is_overline: false,
        font_name: None,
    };

    let mut state = init_state.clone();
    let mut state_stack: Vec<FormatState> = Vec::new();

    let mut raw_lines: Vec<MTextLineInfo> = Vec::new();
    let mut current_spans: Vec<MTextSpanInfo> = Vec::new();
    let mut current_span_text = String::new();
    let mut current_indent = 0.0;
    let mut current_alignment = MTextAlignment::Default;

    let flush_span = |spans: &mut Vec<MTextSpanInfo>, text: &mut String, st: &FormatState| {
        if !text.is_empty() {
            let decoded = decode_cad_text_escapes(text);
            if !decoded.is_empty() {
                spans.push(MTextSpanInfo {
                    text: decoded,
                    is_bold: st.is_bold,
                    is_italic: st.is_italic,
                    height_scale: st.height_scale,
                    width_factor: st.width_factor,
                    tracking_factor: st.tracking_factor,
                    oblique_angle: st.oblique_angle,
                    color_aci: st.color_aci,
                    color_rgb: st.color_rgb,
                    is_underline: st.is_underline,
                    is_strikethrough: st.is_strikethrough,
                    is_overline: st.is_overline,
                    font_name: st.font_name.clone(),
                });
            }
            text.clear();
        }
    };

    let mut chars = normalized.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '{' => {
                flush_span(&mut current_spans, &mut current_span_text, &state);
                state_stack.push(state.clone());
            }
            '}' => {
                flush_span(&mut current_spans, &mut current_span_text, &state);
                if let Some(prev) = state_stack.pop() {
                    state = prev;
                }
            }
            '^' if chars.peek() == Some(&'I') => {
                chars.next(); // consume 'I'
                current_span_text.push_str("    ");
            }
            '\\' => {
                match chars.next() {
                    Some('\\') => current_span_text.push('\\'),
                    Some('{') => current_span_text.push('{'),
                    Some('}') => current_span_text.push('}'),
                    Some('~') => current_span_text.push(' '), // Non-breaking space
                    Some('P') => {
                        // Paragraph / Newline break (uppercase P only!)
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        if let Some(last_span) = current_spans.last_mut() {
                            last_span.text = last_span.text.trim_end().to_string();
                            if last_span.text.is_empty() {
                                current_spans.pop();
                            }
                        }
                        let full_text: String = current_spans.iter().map(|s| s.text.as_str()).collect();
                        let line_h = current_spans.iter().map(|s| s.height_scale).fold(state.height_scale, f64::max);
                        let line_tracking = current_spans.last().map(|s| s.tracking_factor).unwrap_or(state.tracking_factor);
                        let line_wf = current_spans.last().map(|s| s.width_factor).unwrap_or(state.width_factor);
                        let line_bold = current_spans.iter().any(|s| s.is_bold) || state.is_bold;
                        let line_italic = current_spans.iter().any(|s| s.is_italic) || state.is_italic;
                        raw_lines.push(MTextLineInfo {
                            text: full_text,
                            spans: current_spans.clone(),
                            indent: current_indent,
                            width_factor: line_wf,
                            tracking_factor: line_tracking,
                            is_bold: line_bold,
                            is_italic: line_italic,
                            height_scale: line_h,
                            alignment: current_alignment,
                        });
                        current_spans.clear();
                        current_indent = 0.0;
                    }
                    Some('L') => {
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        state.is_underline = true;
                    }
                    Some('l') => {
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        state.is_underline = false;
                    }
                    Some('O') => {
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        state.is_overline = true;
                    }
                    Some('o') => {
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        state.is_overline = false;
                    }
                    Some('K') => {
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        state.is_strikethrough = true;
                    }
                    Some('k') => {
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        state.is_strikethrough = false;
                    }
                    Some('C') => {
                        let mut param = String::new();
                        for next_ch in chars.by_ref() {
                            if next_ch == ';' {
                                break;
                            }
                            param.push(next_ch);
                        }
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        if let Ok(idx) = param.trim().parse::<i32>() {
                            match idx {
                                1..=255 => {
                                    state.color_aci = Some(idx as u8);
                                    state.color_rgb = None;
                                }
                                _ => {
                                    state.color_aci = None;
                                    state.color_rgb = None;
                                }
                            }
                        }
                    }
                    Some('c') => {
                        let mut param = String::new();
                        for next_ch in chars.by_ref() {
                            if next_ch == ';' {
                                break;
                            }
                            param.push(next_ch);
                        }
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        if let Ok(packed) = param.trim().parse::<u32>() {
                            let b = (packed & 0xFF) as u8;
                            let g = ((packed >> 8) & 0xFF) as u8;
                            let r = ((packed >> 16) & 0xFF) as u8;
                            state.color_rgb = Some((r, g, b));
                            state.color_aci = None;
                        }
                    }
                    Some('S') | Some('s') => {
                        // Stacking fraction: \S3#8; -> 3/8, \S1^ 2; -> 1/2
                        let mut frac = String::new();
                        for next_ch in chars.by_ref() {
                            if next_ch == ';' {
                                break;
                            }
                            frac.push(next_ch);
                        }
                        let converted = frac.replace('#', "/").replace('^', "/");
                        current_span_text.push_str(&converted);
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
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        if let Ok(val) = param.trim().parse::<f64>() {
                            if val > 0.05 && val < 10.0 {
                                state.tracking_factor = val;
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
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        if let Ok(val) = param.trim().parse::<f64>() {
                            if val > 0.05 && val < 10.0 {
                                state.width_factor = val;
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
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        if param.contains("|b1") || param.contains("|B1") || param.contains("Bold") || param.contains("bold") {
                            state.is_bold = true;
                        } else if param.contains("|b0") || param.contains("|B0") {
                            state.is_bold = false;
                        }
                        if param.contains("|i1") || param.contains("|I1") || param.contains("Italic") || param.contains("italic") || param.contains("Oblique") {
                            state.is_italic = true;
                        } else if param.contains("|i0") || param.contains("|I0") {
                            state.is_italic = false;
                        }
                        let font_family = param.split('|').next().unwrap_or("").trim();
                        if !font_family.is_empty() {
                            state.font_name = Some(font_family.to_string());
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
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        if let Some(s_str) = param.strip_suffix('x').or_else(|| param.strip_suffix('X')) {
                            if let Ok(scale) = s_str.trim().parse::<f64>() {
                                if scale > 0.01 && scale < 20.0 {
                                    state.height_scale = (state.height_scale * scale).clamp(0.01, 20.0);
                                }
                            }
                        } else if font_height > 0.001 {
                            if let Ok(abs_h) = param.trim().parse::<f64>() {
                                if abs_h > 0.001 {
                                    state.height_scale = (abs_h / font_height).clamp(0.01, 20.0);
                                }
                            }
                        }
                    }
                    Some('Q') | Some('q') => {
                        let mut param = String::new();
                        for next_ch in chars.by_ref() {
                            if next_ch == ';' {
                                break;
                            }
                            param.push(next_ch);
                        }
                        flush_span(&mut current_spans, &mut current_span_text, &state);
                        if let Ok(angle) = param.trim().parse::<f64>() {
                            state.oblique_angle = angle;
                        }
                    }
                    Some('A') | Some('a') => {
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
                                    current_span_text.push(ch);
                                }
                            }
                        }
                    }
                    Some(_) => {}
                    None => {}
                }
            }
            _ => {
                current_span_text.push(ch);
            }
        }
    }

    flush_span(&mut current_spans, &mut current_span_text, &state);
    if let Some(last_span) = current_spans.last_mut() {
        last_span.text = last_span.text.trim_end().to_string();
        if last_span.text.is_empty() {
            current_spans.pop();
        }
    }
    if !current_spans.is_empty() || raw_lines.is_empty() {
        let full_text: String = current_spans.iter().map(|s| s.text.as_str()).collect();
        let line_h = current_spans.iter().map(|s| s.height_scale).fold(state.height_scale, f64::max);
        let line_tracking = current_spans.last().map(|s| s.tracking_factor).unwrap_or(state.tracking_factor);
        let line_wf = current_spans.last().map(|s| s.width_factor).unwrap_or(state.width_factor);
        let line_bold = current_spans.iter().any(|s| s.is_bold) || state.is_bold;
        let line_italic = current_spans.iter().any(|s| s.is_italic) || state.is_italic;
        raw_lines.push(MTextLineInfo {
            text: full_text,
            spans: current_spans,
            indent: current_indent,
            width_factor: line_wf,
            tracking_factor: line_tracking,
            is_bold: line_bold,
            is_italic: line_italic,
            height_scale: line_h,
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
                    spans: Vec::new(),
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
        let str_w: f64 = if !raw.spans.is_empty() {
            raw.spans.iter().map(|s| {
                let sz = font_height * s.height_scale;
                let c_extra = if (s.tracking_factor - 1.0).abs() > 0.01 {
                    (s.tracking_factor - 1.0) * sz * 0.55
                } else {
                    0.0
                };
                let nc = s.text.chars().count();
                if is_shx {
                    (nc as f64) * 0.85 * sz * s.width_factor + c_extra * (nc.saturating_sub(1) as f64)
                } else {
                    estimate_str_width(&s.text, sz) * s.width_factor + c_extra * (nc.saturating_sub(1) as f64)
                }
            }).sum()
        } else {
            estimate_str_width(trimmed, eff_font_h) * raw.width_factor
        };

        let available_w = (rectangle_width - raw.indent).max(eff_font_h);

        // Only wrap if rectangle_width is larger than a single character (otherwise it's an unset/auto width)
        if rectangle_width > eff_font_h * 0.5 && str_w > available_w {
            #[derive(Debug, Clone)]
            struct WordItem {
                text: String,
                spaces_before: usize,
                span: MTextSpanInfo,
            }

            let mut words: Vec<WordItem> = Vec::new();
            let mut pending_spaces: usize = 0;

            for span in &raw.spans {
                let mut current_word = String::new();
                for c in span.text.chars() {
                    if c == ' ' {
                        if !current_word.is_empty() {
                            words.push(WordItem {
                                text: current_word,
                                spaces_before: pending_spaces,
                                span: span.clone(),
                            });
                            current_word = String::new();
                            pending_spaces = 0;
                        }
                        pending_spaces += 1;
                    } else {
                        current_word.push(c);
                    }
                }
                if !current_word.is_empty() {
                    words.push(WordItem {
                        text: current_word,
                        spaces_before: pending_spaces,
                        span: span.clone(),
                    });
                    pending_spaces = 0;
                }
            }

            let mut cur_line_spans: Vec<MTextSpanInfo> = Vec::new();
            let mut cur_w = 0.0;
            let mut is_first_subline = true;

            for word_item in &words {
                let span_h = font_height * word_item.span.height_scale;
                let c_extra = if (word_item.span.tracking_factor - 1.0).abs() > 0.01 {
                    (word_item.span.tracking_factor - 1.0) * span_h * 0.55
                } else {
                    0.0
                };
                let space_w = if is_shx {
                    0.85 * span_h * word_item.span.width_factor + c_extra
                } else {
                    estimate_char_width(' ', span_h) * word_item.span.width_factor + c_extra
                };
                let w_chars = word_item.text.chars().count();
                let word_w = if is_shx {
                    (w_chars as f64) * 0.85 * span_h * word_item.span.width_factor + c_extra * (w_chars.saturating_sub(1) as f64)
                } else {
                    estimate_str_width(&word_item.text, span_h) * word_item.span.width_factor
                        + c_extra * (w_chars.saturating_sub(1) as f64)
                };
                let spaces_w = (word_item.spaces_before as f64) * space_w;
                let target_w = if is_first_subline { available_w } else { rectangle_width };

                if cur_line_spans.is_empty() {
                    if spaces_w >= target_w && target_w > 0.1 {
                        let num_empty = (spaces_w / target_w).floor() as usize;
                        for _ in 0..num_empty {
                            lines.push(MTextLineInfo {
                                text: String::new(),
                                spans: Vec::new(),
                                indent: if is_first_subline { raw.indent } else { 0.0 },
                                width_factor: raw.width_factor,
                                tracking_factor: raw.tracking_factor,
                                is_bold: raw.is_bold,
                                is_italic: raw.is_italic,
                                height_scale: raw.height_scale,
                                alignment: raw.alignment,
                            });
                            is_first_subline = false;
                        }
                        let rem_w = spaces_w - (num_empty as f64) * target_w;
                        cur_w = rem_w + word_w;
                    } else {
                        cur_w = spaces_w + word_w;
                    }
                    let mut span = word_item.span.clone();
                    span.text = word_item.text.clone();
                    cur_line_spans.push(span);
                } else {
                    let needed = spaces_w.max(space_w) + word_w;
                    if cur_w + needed > target_w && target_w > 0.1 {
                        let line_text: String = cur_line_spans.iter().map(|s| s.text.as_str()).collect();
                        let max_h = cur_line_spans.iter().map(|s| s.height_scale).fold(raw.height_scale, f64::max);
                        lines.push(MTextLineInfo {
                            text: line_text,
                            spans: cur_line_spans,
                            indent: if is_first_subline { raw.indent } else { 0.0 },
                            width_factor: raw.width_factor,
                            tracking_factor: raw.tracking_factor,
                            is_bold: raw.is_bold,
                            is_italic: raw.is_italic,
                            height_scale: max_h,
                            alignment: raw.alignment,
                        });
                        cur_line_spans = Vec::new();
                        is_first_subline = false;

                        let target_w_next = rectangle_width;
                        let rem_spaces_w = (spaces_w - space_w).max(0.0);
                        if rem_spaces_w >= target_w_next && target_w_next > 0.1 {
                            let num_empty = (rem_spaces_w / target_w_next).floor() as usize;
                            for _ in 0..num_empty {
                                lines.push(MTextLineInfo {
                                    text: String::new(),
                                    spans: Vec::new(),
                                    indent: 0.0,
                                    width_factor: raw.width_factor,
                                    tracking_factor: raw.tracking_factor,
                                    is_bold: raw.is_bold,
                                    is_italic: raw.is_italic,
                                    height_scale: raw.height_scale,
                                    alignment: raw.alignment,
                                });
                            }
                            let rem_w = rem_spaces_w - (num_empty as f64) * target_w_next;
                            cur_w = rem_w + word_w;
                        } else {
                            cur_w = word_w;
                        }

                        let mut span = word_item.span.clone();
                        span.text = word_item.text.clone();
                        cur_line_spans.push(span);
                    } else {
                        cur_w += needed;
                        let can_merge = cur_line_spans.last().map_or(false, |last| {
                            last.is_bold == word_item.span.is_bold
                                && last.is_italic == word_item.span.is_italic
                                && (last.height_scale - word_item.span.height_scale).abs() < 1e-4
                                && (last.width_factor - word_item.span.width_factor).abs() < 1e-4
                                && (last.tracking_factor - word_item.span.tracking_factor).abs() < 1e-4
                                && (last.oblique_angle - word_item.span.oblique_angle).abs() < 1e-4
                                && last.color_aci == word_item.span.color_aci
                                && last.color_rgb == word_item.span.color_rgb
                                && last.is_underline == word_item.span.is_underline
                                && last.is_strikethrough == word_item.span.is_strikethrough
                                && last.is_overline == word_item.span.is_overline
                                && last.font_name == word_item.span.font_name
                        });

                        if can_merge {
                            let last = cur_line_spans.last_mut().unwrap();
                            last.text.push(' ');
                            last.text.push_str(&word_item.text);
                        } else {
                            let mut new_span = word_item.span.clone();
                            new_span.text = format!(" {}", word_item.text);
                            cur_line_spans.push(new_span);
                        }
                    }
                }
            }

            if !cur_line_spans.is_empty() {
                let line_text: String = cur_line_spans.iter().map(|s| s.text.as_str()).collect();
                let max_h = cur_line_spans.iter().map(|s| s.height_scale).fold(raw.height_scale, f64::max);
                lines.push(MTextLineInfo {
                    text: line_text,
                    spans: cur_line_spans,
                    indent: if is_first_subline { raw.indent } else { 0.0 },
                    width_factor: raw.width_factor,
                    tracking_factor: raw.tracking_factor,
                    is_bold: raw.is_bold,
                    is_italic: raw.is_italic,
                    height_scale: max_h,
                    alignment: raw.alignment,
                });
            }
        } else {
            let mut clean_raw = raw;
            if let Some(last_span) = clean_raw.spans.last_mut() {
                last_span.text = last_span.text.trim_end().to_string();
                if last_span.text.is_empty() {
                    clean_raw.spans.pop();
                }
            }
            clean_raw.text = clean_raw.text.trim_end().to_string();
            lines.push(clean_raw);
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

    #[test]
    fn test_shx_word_wrap() {
        let text = "a word wrap example a b c d e";
        let info = parse_mtext_details_with_shx(text, 1217.828, 100.0, true);
        assert_eq!(info.lines.len(), 3);
        assert_eq!(info.lines[0].text, "a word wrap");
        assert_eq!(info.lines[1].text, "example a b c");
        assert_eq!(info.lines[2].text, "d e");
    }
}
