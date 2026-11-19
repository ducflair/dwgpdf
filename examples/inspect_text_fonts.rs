use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("=== TEXT STYLES IN DOC ===");
    for s in doc.text_styles.iter() {
        println!("Style '{}': font_file='{}', ttf='{}', big_font='{}'", s.name, s.font_file, s.true_type_font, s.big_font_file);
    }

    println!("\n=== TEXTS IN MODEL SPACE ===");
    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        match ent {
            EntityType::Text(t) => {
                if t.value.len() <= 5 {
                    println!("Text #{:<3}: '{}' on layer '{}', style='{}'", i, t.value, t.common.layer, t.style);
                }
            }
            EntityType::MText(mt) => {
                if mt.value.len() <= 5 {
                    println!("MText #{:<3}: '{}' on layer '{}', style='{}'", i, mt.value, mt.common.layer, mt.style);
                }
            }
            _ => {}
        }
    }
}
