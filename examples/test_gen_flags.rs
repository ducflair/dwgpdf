use dwgpdf::cad::load_cad_from_bytes;
use acadrust::entities::EntityType;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dxf/text_mirror_shx_font.dxf").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();
    let mut non_zero = 0;
    for ent in doc.entities() {
        if let EntityType::Text(t) = ent {
            if t.generation_flags != 0 {
                non_zero += 1;
                println!("Found non-zero generation_flags: {} for text '{}'", t.generation_flags, t.value);
            }
        }
    }
    println!("Total non-zero generation_flags in text_mirror_shx_font: {}", non_zero);
}
