use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/sample_AC1014.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        if let EntityType::Hatch(h) = ent {
            if h.gradient_color.enabled {
                println!("\nHatch #{} grad='{}': angle={}, is_single={}, tint={}",
                    i, h.gradient_color.name, h.gradient_color.angle, h.gradient_color.is_single_color, h.gradient_color.color_tint);
                println!("  colors count: {}", h.gradient_color.colors.len());
                for (c_idx, c) in h.gradient_color.colors.iter().enumerate() {
                    println!("    Color #{}: val={}, color={:?}", c_idx, c.value, c.color);
                }
            }
        }
    }
}
