use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/sample_AC1014.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    for ent in doc.entities_in_block("*Model_Space") {
        if let EntityType::Tolerance(tol) = ent {
            println!("Tolerance: dimstyle='{}', pt=({:.2},{:.2})", tol.dimension_style_name, tol.insertion_point.x, tol.insertion_point.y);
        }
    }
}
