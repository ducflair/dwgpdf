use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/sample_AC1014.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("Entities in sample_AC1014.dwg *Model_Space:");
    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        match ent {
            EntityType::Tolerance(tol) => {
                println!("  #{}: Tolerance text='{:?}', pt=({:.2},{:.2}), h={:.2}, gap={:.2}, dir=({:.2},{:.2})",
                    i, tol.text, tol.insertion_point.x, tol.insertion_point.y, tol.text_height, tol.dimension_gap, tol.direction.x, tol.direction.y);
            }
            EntityType::Hatch(h) => {
                println!("  #{}: Hatch name='{}', solid={}, grad_enabled={}, grad_name='{}', grad_angle={}",
                    i, h.pattern.name, h.is_solid, h.gradient_color.enabled, h.gradient_color.name, h.gradient_color.angle);
            }
            EntityType::Text(t) => {
                println!("  #{}: Text '{}' at ({:.2},{:.2})", i, t.value, t.insertion_point.x, t.insertion_point.y);
            }
            EntityType::MText(t) => {
                println!("  #{}: MText '{}' at ({:.2},{:.2})", i, t.value, t.insertion_point.x, t.insertion_point.y);
            }
            _ => {}
        }
    }
}
