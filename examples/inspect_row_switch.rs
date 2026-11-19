use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("Entities near y=-973 (LIGHT SWITCH):");
    for ent in doc.entities_in_block("*Model_Space") {
        match ent {
            EntityType::Text(t) => {
                if (t.insertion_point.y - -973.0).abs() < 500.0 && t.insertion_point.x < 3000.0 {
                    println!("  TEXT: '{}', layer='{}', pt=({:.1}, {:.1})", t.value, t.common.layer, t.insertion_point.x, t.insertion_point.y);
                }
            }
            EntityType::MText(mt) => {
                if (mt.insertion_point.y - -973.0).abs() < 500.0 && mt.insertion_point.x < 3000.0 {
                    println!("  MTEXT: '{}', layer='{}', pt=({:.1}, {:.1})", mt.value, mt.common.layer, mt.insertion_point.x, mt.insertion_point.y);
                }
            }
            EntityType::Insert(ins) => {
                if (ins.insert_point.y - -973.0).abs() < 500.0 && ins.insert_point.x < 3000.0 {
                    println!("  INSERT: block='{}', layer='{}', pt=({:.1}, {:.1})", ins.block_name, ins.common.layer, ins.insert_point.x, ins.insert_point.y);
                }
            }
            EntityType::Line(l) => {
                if (l.start.y - -973.0).abs() < 500.0 && l.start.x < 3000.0 {
                    println!("  LINE: layer='{}', ({:.1}, {:.1}) -> ({:.1}, {:.1})", l.common.layer, l.start.x, l.start.y, l.end.x, l.end.y);
                }
            }
            EntityType::LwPolyline(p) => {
                let first = p.vertices.first().map(|v| (v.location.x, v.location.y)).unwrap_or((0.0, 0.0));
                if (first.1 - -973.0).abs() < 500.0 && first.0 < 3000.0 {
                    println!("  LWPOLY: layer='{}', verts={}", p.common.layer, p.vertices.len());
                }
            }
            EntityType::Circle(c) => {
                if (c.center.y - -973.0).abs() < 500.0 && c.center.x < 3000.0 {
                    println!("  CIRCLE: layer='{}', center=({:.1}, {:.1}), r={:.1}", c.common.layer, c.center.x, c.center.y, c.radius);
                }
            }
            EntityType::Arc(a) => {
                if (a.center.y - -973.0).abs() < 500.0 && a.center.x < 3000.0 {
                    println!("  ARC: layer='{}', center=({:.1}, {:.1}), r={:.1}", a.common.layer, a.center.x, a.center.y, a.radius);
                }
            }
            _ => {}
        }
    }
}
