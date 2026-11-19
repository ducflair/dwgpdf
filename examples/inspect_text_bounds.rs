use dwgpdf::cad::load_cad_from_bytes;
use dwgpdf::cad::bounds::calculate_document_bounds;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dxf/text.dxf").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();
    let b = calculate_document_bounds(&doc, false);
    println!("text.dxf bounds: min=({:.2}, {:.2}), max=({:.2}, {:.2}), w={:.2}, h={:.2}, aspect={:.2}",
        b.min_x, b.min_y, b.max_x, b.max_y, b.width(), b.height(), b.width() / b.height());
}
