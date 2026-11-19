use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let bytes = fs::read(&path).unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("File: {}", path);
    println!("=== BLOCK RECORDS ===");
    for br in doc.block_records.iter() {
        println!("BlockRecord '{}'", br.name);
    }

    println!("\n=== VIEWPORTS ===");
    for ent in doc.entities_in_block("*Model_Space") {
        if let EntityType::Viewport(vp) = ent {
            println!("Model Space VP: id={}, center=({:.1},{:.1}), height={:.1}, frozen_layers={:?}",
                vp.id, vp.center.x, vp.center.y, vp.view_height, vp.frozen_layers);
        }
    }
    for br in doc.block_records.iter() {
        let name = &br.name;
        if name.starts_with("*Paper_Space") {
            for ent in doc.entities_in_block(name) {
                if let EntityType::Viewport(vp) = ent {
                    println!("Paper Space ({}) VP: id={}, center=({:.1},{:.1}), height={:.1}, frozen_layers={:?}",
                        name, vp.id, vp.center.x, vp.center.y, vp.view_height, vp.frozen_layers);
                }
            }
        }
    }
}
