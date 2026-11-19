use dwgpdf::cad::load_cad_from_bytes;
use dwgpdf::cad::bounds::calculate_block_bounds;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/sample_AC1014.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();
    let bb = calculate_block_bounds(&doc, "*Model_Space");
    println!("sample_AC1014 Model_Space bounds: {:?}", bb);
}
