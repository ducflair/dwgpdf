use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    let mut legend_entities = Vec::new();
    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        let (desc, layer, pt) = match ent {
            EntityType::Text(t) => (format!("TEXT '{}'", t.value), &t.common.layer, (t.insertion_point.x, t.insertion_point.y)),
            EntityType::MText(mt) => (format!("MTEXT '{}'", mt.value), &mt.common.layer, (mt.insertion_point.x, mt.insertion_point.y)),
            EntityType::Insert(ins) => (format!("INSERT '{}'", ins.block_name), &ins.common.layer, (ins.insert_point.x, ins.insert_point.y)),
            EntityType::Line(l) => (format!("LINE ({:.1},{:.1})->({:.1},{:.1})", l.start.x, l.start.y, l.end.x, l.end.y), &l.common.layer, (l.start.x, l.start.y)),
            EntityType::Circle(c) => (format!("CIRCLE r={:.1}", c.radius), &c.common.layer, (c.center.x, c.center.y)),
            EntityType::Arc(a) => (format!("ARC r={:.1}", a.radius), &a.common.layer, (a.center.x, a.center.y)),
            EntityType::LwPolyline(p) => (format!("LWPOLY verts={}", p.vertices.len()), &p.common.layer, (p.vertices.first().map(|v| v.location.x).unwrap_or(0.0), p.vertices.first().map(|v| v.location.y).unwrap_or(0.0))),
            other => (format!("{:?}", std::mem::discriminant(other)), &other.common().layer, (0.0, 0.0)),
        };
        if pt.0 >= 0.0 && pt.0 <= 11000.0 && pt.1 >= -37000.0 && pt.1 <= 100.0 {
            legend_entities.push((i, desc, layer.clone(), pt));
        }
    }
    println!("Total legend entities: {}", legend_entities.len());
    for (i, desc, layer, pt) in &legend_entities {
        if pt.0 < 2500.0 { // Left column (symbols)
            println!("Ent #{:<3} at ({:6.1}, {:7.1}) | {:<25} | {}", i, pt.0, pt.1, layer, desc);
        }
    }
}
