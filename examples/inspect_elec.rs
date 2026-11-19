use dwgpdf::cad::load_cad_from_bytes;
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = &args[1];
    let bytes = fs::read(path).expect("failed to read file");
    let doc = load_cad_from_bytes(&bytes).expect("failed to open doc");

    println!("=== LAYERS ===");
    for layer in doc.layers.iter() {
        println!("Layer '{}': is_off={}, is_frozen={}, plottable={}, color={:?}, flags={:?}", 
            layer.name, layer.is_off(), layer.is_frozen(), layer.is_plottable, layer.color, layer.flags);
    }

    println!("\n=== MODEL SPACE INSERTS AND TEXTS ===");
    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        match ent {
            acadrust::entities::EntityType::Insert(ins) => {
                let name = &ins.block_name;
                // Check if this insert is one of the interesting ones
                println!("Insert #{}: block='{}', pt=({:.1}, {:.1}), layer='{}', attrib_count={}", 
                    i, name, ins.insert_point.x, ins.insert_point.y, ins.common.layer, ins.attributes.len());
                for (j, attr) in ins.attributes.iter().enumerate() {
                    println!("   Attr #{}: tag='{}', val='{}', invis={}, layer='{}', pt=({:.1}, {:.1})",
                        j, attr.tag, attr.value, attr.flags.invisible, attr.common.layer, attr.insertion_point.x, attr.insertion_point.y);
                }
            }
            acadrust::entities::EntityType::Text(t) => {
                if t.value.len() < 30 {
                    println!("Text #{}: '{}', layer='{}', pt=({:.1}, {:.1})",
                        i, t.value, t.common.layer, t.insertion_point.x, t.insertion_point.y);
                }
            }
            acadrust::entities::EntityType::MText(mt) => {
                if mt.value.len() < 30 {
                    println!("MText #{}: '{}', layer='{}', pt=({:.1}, {:.1})",
                        i, mt.value, mt.common.layer, mt.insertion_point.x, mt.insertion_point.y);
                }
            }
            _ => {}
        }
    }
}
