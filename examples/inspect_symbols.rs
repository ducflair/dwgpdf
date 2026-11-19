use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    // Find all MText/Text containing "HORN", "INTERCOM", "CCTV", "RISER", "WALL LIGHT", "WEATHERPROOF"
    let targets = ["HORN", "INTERCOM", "CCTV", "RISER", "WALL LIGHT", "WEATHERPROOF"];

    let mut legend_items = Vec::new();

    for ent in doc.entities_in_block("*Model_Space") {
        match ent {
            EntityType::Text(t) => {
                for target in &targets {
                    if t.value.to_uppercase().contains(target) {
                        legend_items.push((t.value.clone(), t.insertion_point.x, t.insertion_point.y, t.common.layer.clone()));
                    }
                }
            }
            EntityType::MText(mt) => {
                for target in &targets {
                    if mt.value.to_uppercase().contains(target) {
                        legend_items.push((mt.value.clone(), mt.insertion_point.x, mt.insertion_point.y, mt.common.layer.clone()));
                    }
                }
            }
            _ => {}
        }
    }

    println!("Found {} target legend texts:", legend_items.len());
    for (text, x, y, layer) in &legend_items {
        println!("\n--- Legend entry: '{}' at ({:.1}, {:.1}) on layer '{}' ---", text, x, y, layer);
        // Now find all entities within y +- 400 and x between 0 and 2500 (where symbols are)
        for ent in doc.entities_in_block("*Model_Space") {
            match ent {
                EntityType::Insert(ins) => {
                    let dy = (ins.insert_point.y - y).abs();
                    if dy < 400.0 && ins.insert_point.x < 3000.0 {
                        println!("  Near Insert: block='{}' at ({:.1}, {:.1}), layer='{}', attribs={}",
                            ins.block_name, ins.insert_point.x, ins.insert_point.y, ins.common.layer, ins.attributes.len());
                        for (i, attr) in ins.attributes.iter().enumerate() {
                            println!("    Attr #{}: tag='{}', val='{}', invis={}, layer='{}', pt=({:.1}, {:.1})",
                                i, attr.tag, attr.value, attr.flags.invisible, attr.common.layer, attr.insertion_point.x, attr.insertion_point.y);
                        }
                        // Also inspect entities inside this block definition!
                        for be in doc.entities_in_block(&ins.block_name) {
                            match be {
                                EntityType::Text(bt) => {
                                    println!("    [Block sub-ent TEXT]: '{}', layer='{}', pt=({:.1}, {:.1})",
                                        bt.value, bt.common.layer, bt.insertion_point.x, bt.insertion_point.y);
                                }
                                EntityType::MText(bmt) => {
                                    println!("    [Block sub-ent MTEXT]: '{}', layer='{}', pt=({:.1}, {:.1})",
                                        bmt.value, bmt.common.layer, bmt.insertion_point.x, bmt.insertion_point.y);
                                }
                                EntityType::AttributeDefinition(attdef) => {
                                    println!("    [Block sub-ent ATTDEF]: tag='{}', default='{}', invis={}, layer='{}'",
                                        attdef.tag, attdef.default_value, attdef.flags.invisible, attdef.common.layer);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                EntityType::Text(t) => {
                    let dy = (t.insertion_point.y - y).abs();
                    if dy < 400.0 && t.insertion_point.x < 2500.0 {
                        println!("  Near Text: '{}' at ({:.1}, {:.1}), layer='{}'",
                            t.value, t.insertion_point.x, t.insertion_point.y, t.common.layer);
                    }
                }
                EntityType::MText(mt) => {
                    let dy = (mt.insertion_point.y - y).abs();
                    if dy < 400.0 && mt.insertion_point.x < 2500.0 {
                        println!("  Near MText: '{}' at ({:.1}, {:.1}), layer='{}'",
                            mt.value, mt.insertion_point.x, mt.insertion_point.y, mt.common.layer);
                    }
                }
                _ => {}
            }
        }
    }
}
