use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("All entity variants near y=-973:");
    for ent in doc.entities_in_block("*Model_Space") {
        match ent {
            EntityType::Line(_) => {} // skip
            EntityType::Text(t) => {
                println!("  TEXT: '{}', layer='{}', pt=({:.1}, {:.1})", t.value, t.common.layer, t.insertion_point.x, t.insertion_point.y);
            }
            other => {
                println!("  Entity: {:?}, layer='{}'", std::mem::discriminant(other), other.common().layer);
            }
        }
    }
}
