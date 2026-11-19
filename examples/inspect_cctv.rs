use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("Entities near y in [-33500, -31500] (Card reader, CCTV, Riser panel):");
    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        let (x, y) = match ent {
            EntityType::Insert(ins) => (ins.insert_point.x, ins.insert_point.y),
            EntityType::Text(t) => (t.insertion_point.x, t.insertion_point.y),
            EntityType::MText(mt) => (mt.insertion_point.x, mt.insertion_point.y),
            _ => continue,
        };
        if y < -31500.0 && y > -33500.0 && x < 2500.0 {
            match ent {
                EntityType::Insert(ins) => {
                    println!("  Insert #{}: block='{}', layer='{}', pt=({:.1}, {:.1})", i, ins.block_name, ins.common.layer, x, y);
                    for be in doc.entities_in_block(&ins.block_name) {
                        match be {
                            EntityType::Text(t) => println!("    [Child Text]: '{}', layer='{}', pt=({:.1}, {:.1})", t.value, t.common.layer, t.insertion_point.x, t.insertion_point.y),
                            EntityType::MText(mt) => println!("    [Child MText]: '{}', layer='{}', pt=({:.1}, {:.1})", mt.value, mt.common.layer, mt.insertion_point.x, mt.insertion_point.y),
                            EntityType::AttributeDefinition(att) => println!("    [Child AttDef]: tag='{}', def='{}', invis={}", att.tag, att.default_value, att.flags.invisible),
                            _ => {}
                        }
                    }
                }
                EntityType::Text(t) => println!("  Text #{}: '{}', layer='{}', pt=({:.1}, {:.1})", i, t.value, t.common.layer, x, y),
                EntityType::MText(mt) => println!("  MText #{}: '{}', layer='{}', pt=({:.1}, {:.1})", i, mt.value, mt.common.layer, x, y),
                _ => {}
            }
        }
    }
}
