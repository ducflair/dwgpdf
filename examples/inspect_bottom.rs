use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("All entities with y < -30000 and x < 3000:");
    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        match ent {
            EntityType::Insert(ins) => {
                if ins.insert_point.y < -30000.0 && ins.insert_point.x < 3000.0 {
                    println!("  Insert #{}: block='{}', layer='{}', pt=({:.1}, {:.1})", i, ins.block_name, ins.common.layer, ins.insert_point.x, ins.insert_point.y);
                }
            }
            EntityType::Text(t) => {
                if t.insertion_point.y < -30000.0 && t.insertion_point.x < 3000.0 {
                    println!("  Text #{}: '{}', layer='{}', pt=({:.1}, {:.1})", i, t.value, t.common.layer, t.insertion_point.x, t.insertion_point.y);
                }
            }
            EntityType::MText(mt) => {
                if mt.insertion_point.y < -30000.0 && mt.insertion_point.x < 3000.0 {
                    println!("  MText #{}: '{}', layer='{}', pt=({:.1}, {:.1})", i, mt.value, mt.common.layer, mt.insertion_point.x, mt.insertion_point.y);
                }
            }
            _ => {}
        }
    }
}
