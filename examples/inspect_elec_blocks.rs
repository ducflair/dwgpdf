use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    let target_blocks = ["SP3", "INTERCOM", "CTV", "DL", "LIGHT-07", "EX-G3", "WEATHERPROOF WP", "SEN"];
    for bname in &target_blocks {
        println!("\n=== Block '{}' ===", bname);
        for ent in doc.entities_in_block(bname) {
            match ent {
                EntityType::Text(t) => println!("  Text: '{}', layer='{}', pt=({:.1}, {:.1})", t.value, t.common.layer, t.insertion_point.x, t.insertion_point.y),
                EntityType::MText(mt) => println!("  MText: '{}', layer='{}', pt=({:.1}, {:.1})", mt.value, mt.common.layer, mt.insertion_point.x, mt.insertion_point.y),
                EntityType::AttributeDefinition(att) => println!("  AttDef: tag='{}', prompt='{}', def='{}', layer='{}', invis={}", att.tag, att.prompt, att.default_value, att.common.layer, att.flags.invisible),
                EntityType::Line(l) => println!("  Line: layer='{}', ({:.1}, {:.1})->({:.1}, {:.1})", l.common.layer, l.start.x, l.start.y, l.end.x, l.end.y),
                EntityType::Circle(c) => println!("  Circle: layer='{}', center=({:.1}, {:.1}), r={:.1}", c.common.layer, c.center.x, c.center.y, c.radius),
                EntityType::Arc(a) => println!("  Arc: layer='{}', center=({:.1}, {:.1}), r={:.1}", a.common.layer, a.center.x, a.center.y, a.radius),
                other => println!("  Other: {:?}, layer='{}'", std::mem::discriminant(other), other.common().layer),
            }
        }
    }
}
