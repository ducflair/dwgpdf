use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("All short Text/MText in *Model_Space:");
    for ent in doc.entities_in_block("*Model_Space") {
        match ent {
            EntityType::Text(t) => {
                if t.value.len() <= 6 {
                    println!("  TEXT: '{}', layer='{}', pt=({:.1}, {:.1})", t.value, t.common.layer, t.insertion_point.x, t.insertion_point.y);
                }
            }
            EntityType::MText(mt) => {
                if mt.value.len() <= 10 {
                    println!("  MTEXT: '{}', layer='{}', pt=({:.1}, {:.1})", mt.value, mt.common.layer, mt.insertion_point.x, mt.insertion_point.y);
                }
            }
            _ => {}
        }
    }
}
