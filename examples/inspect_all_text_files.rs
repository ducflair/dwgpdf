use dwgpdf::cad::load_cad_from_bytes;
use acadrust::entities::EntityType;
use std::fs;

fn analyze_file(path_str: &str) {
    println!("\n=======================================================");
    println!("ANALYZING: {}", path_str);
    println!("=======================================================");
    let bytes = match fs::read(path_str) {
        Ok(b) => b,
        Err(e) => {
            println!("Could not read: {}", e);
            return;
        }
    };
    let doc = match load_cad_from_bytes(&bytes) {
        Ok(d) => d,
        Err(e) => {
            println!("Could not parse CAD: {:?}", e);
            return;
        }
    };

    println!("--- TEXT STYLES ({}) ---", doc.text_styles.len());
    for s in doc.text_styles.iter() {
        println!("  Style '{}': font='{}', big_font='{}', ttf='{}', height={:.2}, wf={:.2}, obl={:.2}, flags={:?}",
            s.name, s.font_file, s.big_font_file, s.true_type_font, s.height, s.width_factor, s.oblique_angle, s.flags);
    }

    let mut text_count = 0;
    let mut mtext_count = 0;
    let mut other_count = 0;

    for (i, ent) in doc.entities().enumerate() {
        match ent {
            EntityType::Text(t) => {
                text_count += 1;
                if text_count <= 25 {
                    println!("  [TEXT #{}] val='{}', style='{}', h={:.2}, rot={:.2}, wf={:.2}, obl={:.2}, h_al={:?}, v_al={:?}, gen_flags={}, p1=({:.2},{:.2}), p2={:?}, norm=({:.2},{:.2},{:.2})",
                        i, t.value, t.style, t.height, t.rotation, t.width_factor, t.oblique_angle,
                        t.horizontal_alignment, t.vertical_alignment, t.generation_flags,
                        t.insertion_point.x, t.insertion_point.y,
                        t.alignment_point.as_ref().map(|p| (p.x, p.y)),
                        t.normal.x, t.normal.y, t.normal.z
                    );
                }
            }
            EntityType::MText(m) => {
                mtext_count += 1;
                if mtext_count <= 25 {
                    println!("  [MTEXT #{}] val='{}', style='{}', h={:.2}, rot={:.2}, rect_w={:.2}, attach={:?}, dir={:?}, norm=({:.2},{:.2},{:.2})",
                        i, m.value, m.style, m.height, m.rotation, m.rectangle_width,
                        m.attachment_point, m.drawing_direction,
                        m.normal.x, m.normal.y, m.normal.z
                    );
                }
            }
            _ => { other_count += 1; }
        }
    }
    println!("Summary: {} TEXT, {} MTEXT, {} other entities", text_count, mtext_count, other_count);
}

fn main() {
    let files = [
        "tests/fixtures/dxf/smoke-plain.dxf",
        "tests/fixtures/dxf/text.dxf",
        "tests/fixtures/dxf/text_fonts.dxf",
        "tests/fixtures/dxf/text_mirror_shx_font.dxf",
        "tests/fixtures/dxf/text_mirror_true_type_font.dxf",
        "tests/fixtures/dxf/text_negative_extrusion.dxf",
        "tests/fixtures/dxf/uncommon.dxf",
    ];
    for f in &files {
        analyze_file(f);
    }
}
