use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/sample_AC1014.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        if let EntityType::Hatch(h) = ent {
            if h.gradient_color.enabled {
                println!("Hatch #{} grad='{}': paths={}", i, h.gradient_color.name, h.paths.len());
                for (p_idx, p) in h.paths.iter().enumerate() {
                    println!("  Path #{}: edges={}", p_idx, p.edges.len());
                }
            }
        }
    }
}
