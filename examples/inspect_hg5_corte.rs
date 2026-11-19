use acadrust::document::CadDocument;
use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn inspect_wipeouts(doc: &CadDocument, block_name: &str) {
    for ent in doc.entities_in_block(block_name) {
        if let EntityType::Wipeout(w) = ent {
            println!("Block '{}' WIPEOUT handle={:?} layer='{}':\n{:#?}", block_name, ent.common().handle, ent.as_entity().layer(), w);
        }
    }
}

fn main() {
    let bytes = fs::read("tests/fixtures/dxf/wipeout_door.dxf").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    for br in doc.block_records.iter() {
        inspect_wipeouts(&doc, &br.name);
    }
}
