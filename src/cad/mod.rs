pub mod bounds;
pub mod color;
pub mod linetype;
pub mod mtext;

use crate::error::{ConversionError, ConversionResult};
use acadrust::document::CadDocument;
use acadrust::io::dwg::DwgReader;
use acadrust::io::dxf::DxfReader;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

/// Detects CAD file format from bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CadFormat {
    Dwg,
    Dxf,
    Dwf,
    Unknown,
}

pub fn detect_format(bytes: &[u8]) -> CadFormat {
    if bytes.starts_with(b"(DWF V") || bytes.starts_with(b"PK\x03\x04") {
        CadFormat::Dwf
    } else if bytes.starts_with(b"AC10") {
        // AutoCAD DWG files begin with AC10xx
        CadFormat::Dwg
    } else if bytes.len() > 6 && (bytes.starts_with(b"0\n") || bytes.starts_with(b"0\r\n") || bytes.windows(7).any(|w| w == b"SECTION")) {
        CadFormat::Dxf
    } else {
        // Fallback: check if ASCII text with DXF keywords
        let preview = String::from_utf8_lossy(&bytes[..512.min(bytes.len())]);
        if preview.contains("SECTION") || preview.contains("HEADER") || preview.contains("ENTITIES") {
            CadFormat::Dxf
        } else {
            CadFormat::Unknown
        }
    }
}

#[derive(Default)]
struct AttdefDxfData {
    flags: Option<i16>,
    h_align: Option<acadrust::entities::attribute_definition::HorizontalAlignment>,
    v_align: Option<acadrust::entities::attribute_definition::VerticalAlignment>,
    width_factor: Option<f64>,
    oblique_angle: Option<f64>,
    text_style: Option<String>,
    align_x: Option<f64>,
    align_y: Option<f64>,
    align_z: Option<f64>,
    norm_x: Option<f64>,
    norm_y: Option<f64>,
    norm_z: Option<f64>,
}

fn enrich_dxf_blocks(bytes: &[u8], doc: &mut CadDocument) {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();

    let mut in_blocks_section = false;
    let mut in_block = false;
    let mut current_block_name: Option<String> = None;
    let mut current_bx: Option<f64> = None;
    let mut current_by: Option<f64> = None;
    let mut current_bz: Option<f64> = None;
    let mut block_base_points: HashMap<String, acadrust::Vector3> = HashMap::new();

    while let Some(code_line) = lines.next() {
        let code_str = code_line.trim();
        if code_str.is_empty() {
            continue;
        }
        let code: i32 = match code_str.parse() {
            Ok(c) => c,
            Err(_) => continue,
        };
        let val_line = match lines.next() {
            Some(v) => v.trim(),
            None => break,
        };

        if code == 0 {
            if val_line.eq_ignore_ascii_case("SECTION") {
                continue;
            }
            if val_line.eq_ignore_ascii_case("ENDSEC") {
                if in_block {
                    if let Some(name) = current_block_name.take() {
                        let bp = acadrust::Vector3::new(
                            current_bx.unwrap_or(0.0),
                            current_by.unwrap_or(0.0),
                            current_bz.unwrap_or(0.0),
                        );
                        block_base_points.insert(name, bp);
                    }
                    in_block = false;
                }
                in_blocks_section = false;
                continue;
            }
            if in_block {
                if let Some(name) = current_block_name.take() {
                    let bp = acadrust::Vector3::new(
                        current_bx.unwrap_or(0.0),
                        current_by.unwrap_or(0.0),
                        current_bz.unwrap_or(0.0),
                    );
                    block_base_points.insert(name, bp);
                }
                in_block = false;
            }
            if in_blocks_section && val_line.eq_ignore_ascii_case("BLOCK") {
                in_block = true;
                current_block_name = None;
                current_bx = None;
                current_by = None;
                current_bz = None;
            }
            continue;
        }

        if code == 2 && !in_blocks_section && val_line.eq_ignore_ascii_case("BLOCKS") {
            in_blocks_section = true;
            continue;
        }

        if in_block {
            match code {
                2 => {
                    if current_block_name.is_none() {
                        current_block_name = Some(val_line.to_string());
                    }
                }
                10 => {
                    if let Ok(v) = val_line.parse::<f64>() {
                        current_bx = Some(v);
                    }
                }
                20 => {
                    if let Ok(v) = val_line.parse::<f64>() {
                        current_by = Some(v);
                    }
                }
                30 => {
                    if let Ok(v) = val_line.parse::<f64>() {
                        current_bz = Some(v);
                    }
                }
                _ => {}
            }
        }
    }

    for (name, bp) in block_base_points {
        if let Some(br) = doc.block_records.get_mut(&name) {
            br.base_point = bp;
        }
    }
}

fn enrich_dxf_mtexts(bytes: &[u8], doc: &mut CadDocument) {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();

    let mut dict_to_owner: HashMap<u64, u64> = HashMap::new();
    let mut xrec_to_mtext: HashMap<u64, u64> = HashMap::new();
    let mut xrec_texts: HashMap<u64, String> = HashMap::new();

    let mut in_xdict = false;
    let mut current_obj = String::new();
    let mut current_handle: Option<u64> = None;
    let mut current_owner: Option<u64> = None;
    let mut last_dict_key = String::new();

    while let Some(code_line) = lines.next() {
        let code_str = code_line.trim();
        if code_str.is_empty() {
            continue;
        }
        let code: i32 = match code_str.parse() {
            Ok(c) => c,
            Err(_) => continue,
        };
        let val_line = match lines.next() {
            Some(v) => v.trim(),
            None => break,
        };

        if code == 0 {
            current_obj = val_line.to_string();
            current_handle = None;
            current_owner = None;
            last_dict_key.clear();
            in_xdict = false;
            continue;
        }

        if code == 5 {
            if let Ok(h) = u64::from_str_radix(val_line, 16) {
                current_handle = Some(h);
            }
            continue;
        }

        if code == 330 && current_owner.is_none() {
            if let Ok(h) = u64::from_str_radix(val_line, 16) {
                current_owner = Some(h);
            }
            continue;
        }

        if code == 102 {
            if val_line.eq_ignore_ascii_case("{ACAD_XDICTIONARY") {
                in_xdict = true;
            } else if val_line == "}" {
                in_xdict = false;
            }
            continue;
        }

        if in_xdict && code == 360 {
            if let (Some(owner), Ok(dict_h)) = (current_handle, u64::from_str_radix(val_line, 16)) {
                dict_to_owner.insert(dict_h, owner);
            }
            continue;
        }

        if current_obj.eq_ignore_ascii_case("DICTIONARY") {
            if code == 3 {
                last_dict_key = val_line.to_string();
            } else if code == 360 {
                if last_dict_key.eq_ignore_ascii_case("ACAD_MTEXT_2008_RT") {
                    if let Ok(xrec_h) = u64::from_str_radix(val_line, 16) {
                        let owner_h = current_owner.or_else(|| current_handle.and_then(|dh| dict_to_owner.get(&dh).copied()));
                        if let Some(owner) = owner_h {
                            xrec_to_mtext.insert(xrec_h, owner);
                        }
                    }
                }
                last_dict_key.clear();
            }
            continue;
        }

        if current_obj.eq_ignore_ascii_case("XRECORD") {
            if code == 1 || code == 3 {
                if let Some(h) = current_handle {
                    xrec_texts.entry(h).or_default().push_str(val_line);
                }
            }
            continue;
        }
    }

    let mut mtext_rich_texts: HashMap<u64, String> = HashMap::new();
    for (xrec_h, mtext_h) in xrec_to_mtext {
        if let Some(text) = xrec_texts.get(&xrec_h) {
            mtext_rich_texts.insert(mtext_h, text.clone());
        }
    }

    if mtext_rich_texts.is_empty() {
        return;
    }

    for ent in doc.entities_mut() {
        if let acadrust::entities::EntityType::MText(mtext) = ent {
            let h = mtext.common.handle.value();
            if let Some(rich) = mtext_rich_texts.get(&h) {
                if mtext.value.is_empty() || rich.len() > mtext.value.len() {
                    mtext.value = rich.clone();
                }
            }
        }
    }
}

fn enrich_dxf_attdefs(bytes: &[u8], doc: &mut CadDocument) {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let mut attdefs: HashMap<u64, AttdefDxfData> = HashMap::new();

    let mut in_attdef = false;
    let mut current_handle: Option<u64> = None;
    let mut current_data = AttdefDxfData::default();

    while let Some(code_line) = lines.next() {
        let code_str = code_line.trim();
        if code_str.is_empty() {
            continue;
        }
        let code: i32 = match code_str.parse() {
            Ok(c) => c,
            Err(_) => continue,
        };
        let val_line = match lines.next() {
            Some(v) => v.trim(),
            None => break,
        };

        if code == 0 {
            if in_attdef {
                if let Some(h) = current_handle {
                    attdefs.insert(h, std::mem::take(&mut current_data));
                }
            }
            if val_line.eq_ignore_ascii_case("ATTDEF") || val_line.eq_ignore_ascii_case("ATTRIB") {
                in_attdef = true;
                current_handle = None;
                current_data = AttdefDxfData::default();
            } else {
                in_attdef = false;
            }
            continue;
        }

        if in_attdef {
            match code {
                5 => {
                    if let Ok(h) = u64::from_str_radix(val_line, 16) {
                        current_handle = Some(h);
                    }
                }
                7 => {
                    current_data.text_style = Some(val_line.to_string());
                }
                11 => {
                    if let Ok(v) = val_line.parse::<f64>() {
                        current_data.align_x = Some(v);
                    }
                }
                21 => {
                    if let Ok(v) = val_line.parse::<f64>() {
                        current_data.align_y = Some(v);
                    }
                }
                31 => {
                    if let Ok(v) = val_line.parse::<f64>() {
                        current_data.align_z = Some(v);
                    }
                }
                41 => {
                    if let Ok(v) = val_line.parse::<f64>() {
                        current_data.width_factor = Some(v);
                    }
                }
                51 => {
                    if let Ok(v) = val_line.parse::<f64>() {
                        current_data.oblique_angle = Some(v.to_radians());
                    }
                }
                71 => {
                    if let Ok(v) = val_line.parse::<i16>() {
                        current_data.flags = Some(v);
                    }
                }
                72 => {
                    if let Ok(v) = val_line.parse::<i16>() {
                        current_data.h_align = Some(acadrust::entities::attribute_definition::HorizontalAlignment::from_value(v));
                    }
                }
                74 => {
                    if let Ok(v) = val_line.parse::<i16>() {
                        current_data.v_align = Some(acadrust::entities::attribute_definition::VerticalAlignment::from_value(v));
                    }
                }
                210 => {
                    if let Ok(v) = val_line.parse::<f64>() {
                        current_data.norm_x = Some(v);
                    }
                }
                220 => {
                    if let Ok(v) = val_line.parse::<f64>() {
                        current_data.norm_y = Some(v);
                    }
                }
                230 => {
                    if let Ok(v) = val_line.parse::<f64>() {
                        current_data.norm_z = Some(v);
                    }
                }
                _ => {}
            }
        }
    }

    if in_attdef {
        if let Some(h) = current_handle {
            attdefs.insert(h, current_data);
        }
    }

    let apply_attdef_data = |attdef: &mut acadrust::entities::AttributeDefinition| {
        let h = attdef.common.handle.value();
        if let Some(data) = attdefs.get(&h) {
            if data.norm_x.is_some() || data.norm_y.is_some() || data.norm_z.is_some() {
                attdef.normal = acadrust::Vector3::new(
                    data.norm_x.unwrap_or(0.0),
                    data.norm_y.unwrap_or(0.0),
                    data.norm_z.unwrap_or(1.0),
                );
            }
            if let Some(flags) = data.flags {
                attdef.text_generation_flags = flags;
            }
            if let Some(h_align) = data.h_align {
                attdef.horizontal_alignment = h_align;
            }
            if let Some(v_align) = data.v_align {
                attdef.vertical_alignment = v_align;
            }
            if let Some(wf) = data.width_factor {
                attdef.width_factor = wf;
            }
            if let Some(obl) = data.oblique_angle {
                attdef.oblique_angle = obl;
            }
            if let Some(ref style) = data.text_style {
                attdef.text_style = style.clone();
            }
            if data.align_x.is_some() || data.align_y.is_some() {
                let mut ap = attdef.alignment_point;
                if let Some(x) = data.align_x { ap.x = x; }
                if let Some(y) = data.align_y { ap.y = y; }
                if let Some(z) = data.align_z { ap.z = z; }
                attdef.alignment_point = ap;
            }
        }
    };

    let apply_attrib_data = |attrib: &mut acadrust::entities::AttributeEntity| {
        attrib.rotation = attrib.rotation.to_radians();
        attrib.oblique_angle = attrib.oblique_angle.to_radians();
        let h = attrib.common.handle.value();
        if let Some(data) = attdefs.get(&h) {
            if data.norm_x.is_some() || data.norm_y.is_some() || data.norm_z.is_some() {
                attrib.normal = acadrust::Vector3::new(
                    data.norm_x.unwrap_or(0.0),
                    data.norm_y.unwrap_or(0.0),
                    data.norm_z.unwrap_or(1.0),
                );
            }
            if let Some(flags) = data.flags {
                attrib.text_generation_flags = flags;
            }
            if let Some(h_align) = data.h_align {
                attrib.horizontal_alignment = h_align;
            }
            if let Some(v_align) = data.v_align {
                attrib.vertical_alignment = v_align;
            }
            if let Some(wf) = data.width_factor {
                attrib.width_factor = wf;
            }
            if let Some(obl) = data.oblique_angle {
                attrib.oblique_angle = obl;
            }
            if let Some(ref style) = data.text_style {
                attrib.text_style = style.clone();
            }
            if data.align_x.is_some() || data.align_y.is_some() {
                let mut ap = attrib.alignment_point;
                if let Some(x) = data.align_x { ap.x = x; }
                if let Some(y) = data.align_y { ap.y = y; }
                if let Some(z) = data.align_z { ap.z = z; }
                attrib.alignment_point = ap;
            }
        }
    };

    for ent in doc.entities_mut() {
        match ent {
            acadrust::entities::EntityType::AttributeDefinition(attdef) => {
                apply_attdef_data(attdef);
            }
            acadrust::entities::EntityType::AttributeEntity(attrib) => {
                apply_attrib_data(attrib);
            }
            acadrust::entities::EntityType::Insert(insert) => {
                for attrib in &mut insert.attributes {
                    apply_attrib_data(attrib);
                }
            }
            _ => {}
        }
    }
}

/// Loads a CAD document (DWG or DXF) from an in-memory buffer.
pub fn load_cad_from_bytes(bytes: &[u8]) -> ConversionResult<CadDocument> {
    let format = detect_format(bytes);
    let mut doc = match format {
        CadFormat::Dwg => {
            let cursor = Cursor::new(bytes.to_vec());
            let mut reader = DwgReader::from_stream(cursor);
            reader.read().map_err(|e| ConversionError::DwgParse(e.to_string()))?
        }
        CadFormat::Dxf | CadFormat::Unknown => {
            let cursor = Cursor::new(bytes.to_vec());
            let reader = DxfReader::from_reader(cursor).map_err(|e| ConversionError::DxfParse(e.to_string()))?;
            let mut doc = reader.read().map_err(|e| ConversionError::DxfParse(e.to_string()))?;
            enrich_dxf_attdefs(bytes, &mut doc);
            enrich_dxf_blocks(bytes, &mut doc);
            enrich_dxf_mtexts(bytes, &mut doc);
            doc
        }
        CadFormat::Dwf => {
            return Err(ConversionError::CadRead("DWF format should be processed by DWF module".to_string()));
        }
    };

    // Standard CAD layer plottability resolution:
    // In AutoCAD, all layers are plottable by default. The plot flag (DXF 290) was only introduced
    // in AutoCAD 2000 (AC1015). If no layers in the document have is_plottable == true (e.g. pre-R2000 DWG/DXF
    // where plot flags were omitted or uninitialized by the reader), default non-defpoints/non-nplt layers to plottable.
    if !doc.layers.iter().any(|l| l.is_plottable) {
        for layer in doc.layers.iter_mut() {
            if !layer.name.eq_ignore_ascii_case("defpoints") && !layer.name.to_lowercase().contains("nplt") {
                layer.is_plottable = true;
            }
        }
    }

    Ok(doc)
}

/// Loads a CAD document from a file path.
pub fn load_cad_file(path: &Path) -> ConversionResult<CadDocument> {
    let bytes = std::fs::read(path).map_err(ConversionError::Io)?;
    load_cad_from_bytes(&bytes)
}
