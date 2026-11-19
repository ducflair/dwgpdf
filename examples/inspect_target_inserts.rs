use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    let target_blocks = ["AC POWER 2", "PROJECTOR HORN", "INTERCOM", "tv", "WEATHERPROOF WP", "DL", "DUCT PROBE", "EWIS"];

    println!("Target Inserts in ModelSpace:");
    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        if let EntityType::Insert(ins) = ent {
            if target_blocks.contains(&ins.block_name.as_str()) {
                println!("Insert #{}: block='{}', layer='{}', pt=({:.1}, {:.1})",
                    i, ins.block_name, ins.common.layer, ins.insert_point.x, ins.insert_point.y);
            }
        }
    }
}
