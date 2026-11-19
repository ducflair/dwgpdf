use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    let target_texts = ["PR", "PROBE", "MOD", "STR", "I", "B", "X", "LED", "s", "P", "S", "WP", "WPE", "PTV", "TV", "NTD", "EWIS", "FIP", "MIMIC", "DB", "wo", "AC", "FAN", "AB", "GI", "RH", "DW", "MW", "F", "PIR", "D", "FLE", "FL1"];

    println!("=== SEARCH IN MODEL SPACE ENTITIES ===");
    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        match ent {
            EntityType::Text(t) => {
                if target_texts.contains(&t.value.as_str()) {
                    println!("ModelSpace Text #{:<3}: '{}' on layer '{}' at ({:.1}, {:.1})", i, t.value, t.common.layer, t.insertion_point.x, t.insertion_point.y);
                }
            }
            EntityType::MText(mt) => {
                let trimmed = mt.value.trim();
                for &target in &target_texts {
                    if trimmed.contains(target) {
                        println!("ModelSpace MText #{:<3}: '{}' on layer '{}' at ({:.1}, {:.1})", i, trimmed, mt.common.layer, mt.insertion_point.x, mt.insertion_point.y);
                    }
                }
            }
            EntityType::Insert(ins) => {
                for (a_idx, attr) in ins.attributes.iter().enumerate() {
                    if target_texts.contains(&attr.value.as_str()) {
                        println!("ModelSpace Insert #{:<3} ('{}' on '{}' at ({:.1},{:.1})) Attr #{}: tag='{}', val='{}' on layer '{}'",
                            i, ins.block_name, ins.common.layer, ins.insert_point.x, ins.insert_point.y, a_idx, attr.tag, attr.value, attr.common.layer);
                    }
                }
            }
            _ => {}
        }
    }

    println!("\n=== SEARCH IN ALL BLOCK DEFINITIONS ===");
    for br in doc.block_records.iter() {
        for ent in doc.entities_in_block(&br.name) {
            match ent {
                EntityType::Text(t) => {
                    if target_texts.contains(&t.value.as_str()) {
                        println!("Block '{}' Text: '{}' on layer '{}' at ({:.1}, {:.1})", br.name, t.value, t.common.layer, t.insertion_point.x, t.insertion_point.y);
                    }
                }
                EntityType::MText(mt) => {
                    let trimmed = mt.value.trim();
                    for &target in &target_texts {
                        if trimmed.contains(target) {
                            println!("Block '{}' MText: '{}' on layer '{}' at ({:.1}, {:.1})", br.name, trimmed, mt.common.layer, mt.insertion_point.x, mt.insertion_point.y);
                        }
                    }
                }
                EntityType::AttributeDefinition(att) => {
                    if target_texts.contains(&att.default_value.as_str()) || target_texts.contains(&att.tag.as_str()) {
                        println!("Block '{}' AttDef: tag='{}', def='{}' on layer '{}' invis={}", br.name, att.tag, att.default_value, att.common.layer, att.flags.invisible);
                    }
                }
                _ => {}
            }
        }
    }
}
