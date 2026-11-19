use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    // Collect all legend text rows (x around 3000, y from 0 down to -36000)
    let mut rows: Vec<(f64, String)> = Vec::new();
    for ent in doc.entities_in_block("*Model_Space") {
        match ent {
            EntityType::Text(t) => {
                if t.insertion_point.x > 2500.0 && t.insertion_point.x < 4000.0 {
                    rows.push((t.insertion_point.y, t.value.clone()));
                }
            }
            EntityType::MText(mt) => {
                if mt.insertion_point.x > 2500.0 && mt.insertion_point.x < 4000.0 {
                    rows.push((mt.insertion_point.y, mt.value.clone()));
                }
            }
            _ => {}
        }
    }
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    for (y, label) in &rows {
        // Find entities with x < 2500 and |ent.y - y| < 300
        let mut symbols = Vec::new();
        for ent in doc.entities_in_block("*Model_Space") {
            match ent {
                EntityType::Insert(ins) => {
                    if (ins.insert_point.y - y).abs() < 300.0 && ins.insert_point.x < 2500.0 {
                        let attrs: Vec<String> = ins.attributes.iter().map(|a| format!("{}='{}'", a.tag, a.value)).collect();
                        symbols.push(format!("INSERT '{}' (layer={}, attrs=[{}])", ins.block_name, ins.common.layer, attrs.join(", ")));
                    }
                }
                EntityType::Text(t) => {
                    if (t.insertion_point.y - y).abs() < 300.0 && t.insertion_point.x < 2500.0 {
                        symbols.push(format!("TEXT '{}' (layer={})", t.value, t.common.layer));
                    }
                }
                EntityType::MText(mt) => {
                    if (mt.insertion_point.y - y).abs() < 300.0 && mt.insertion_point.x < 2500.0 {
                        symbols.push(format!("MTEXT '{}' (layer={})", mt.value, mt.common.layer));
                    }
                }
                _ => {}
            }
        }
        println!("y={:.1} | {:<55} | {}", y, label, symbols.join(" ; "));
    }
}
