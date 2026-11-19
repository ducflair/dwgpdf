use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/example_2000.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("Entities in block 'CIRKLO_PUNKTOJ':");
    for (i, ent) in doc.entities_in_block("CIRKLO_PUNKTOJ").enumerate() {
        println!("  #{}: type={:?}, layer='{}'", i, std::mem::discriminant(ent), ent.as_entity().layer());
        match ent {
            EntityType::Circle(c) => println!("    Circle center=({:.2},{:.2}), r={:.2}", c.center.x, c.center.y, c.radius),
            EntityType::Arc(a) => println!("    Arc center=({:.2},{:.2}), r={:.2}, ang=({:.1}..{:.1})", a.center.x, a.center.y, a.radius, a.start_angle.to_degrees(), a.end_angle.to_degrees()),
            EntityType::Point(p) => println!("    Point at ({:.2},{:.2})", p.location.x, p.location.y),
            EntityType::Line(l) => println!("    Line ({:.2},{:.2}) -> ({:.2},{:.2})", l.start.x, l.start.y, l.end.x, l.end.y),
            EntityType::LwPolyline(lp) => println!("    LwPolyline verts={}", lp.vertices.len()),
            _ => {}
        }
    }

    for br in doc.block_records.iter() {
        if br.name == "CIRKLO_PUNKTOJ" {
            println!("BlockRecord CIRKLO_PUNKTOJ base_point=({:.2},{:.2})", br.base_point.x, br.base_point.y);
        }
    }
}
