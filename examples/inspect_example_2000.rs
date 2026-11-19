use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/example_2000.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("Layouts:");
    for (_h, obj) in &doc.objects {
        if let acadrust::objects::ObjectType::Layout(l) = obj {
            println!("  Layout '{}': tab={}, paper=({}x{}), limits=({:?}) to ({:?})",
                l.name, l.tab_order, l.paper_width, l.paper_height, l.min_limits, l.max_limits);
        }
    }

    println!("\nBounds without #62:");
    let mut b_all = dwgpdf::cad::bounds::BoundingBox::new();
    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        if i == 62 { continue; }
        let mut b = dwgpdf::cad::bounds::BoundingBox::new();
        let mut visited = std::collections::HashSet::new();
        dwgpdf::cad::bounds::accumulate_entity_bounds(&doc, ent, &dwgpdf::cad::bounds::Transform2D::identity(), &mut b, &mut visited, 0, None);
        if b.is_valid() {
            b_all.update_bbox(&b);
        }
    }
    println!("  Without #62: min=({:.2},{:.2}), max=({:.2},{:.2}), w={:.2}, h={:.2}",
        b_all.min_x, b_all.min_y, b_all.max_x, b_all.max_y, b_all.width(), b_all.height());
    for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        match ent {
            EntityType::Circle(c) => {
                println!("  #{}: Circle center=({:.2},{:.2}), r={:.2}, layer='{}'",
                    i, c.center.x, c.center.y, c.radius, c.common.layer);
            }
            EntityType::Arc(a) => {
                println!("  #{}: Arc center=({:.2},{:.2}), r={:.2}, ang=({:.1}..{:.1}), layer='{}'",
                    i, a.center.x, a.center.y, a.radius, a.start_angle.to_degrees(), a.end_angle.to_degrees(), a.common.layer);
            }
            EntityType::LwPolyline(lp) => {
                println!("  #{}: LwPolyline verts={}, closed={}, layer='{}'",
                    i, lp.vertices.len(), lp.is_closed, lp.common.layer);
            }
            EntityType::Viewport(vp) => {
                println!("  #{}: Viewport center=({:.2},{:.2}), w={:.2}, h={:.2}",
                    i, vp.center.x, vp.center.y, vp.width, vp.height);
            }
            EntityType::Insert(ins) => {
                println!("  #{}: Insert '{}' at ({:.2},{:.2}), layer='{}'",
                    i, ins.block_name, ins.insert_point.x, ins.insert_point.y, ins.common.layer);
            }
            _ => {
                println!("  #{}: Other ({:?}) layer='{}'", i, std::mem::discriminant(ent), ent.as_entity().layer());
            }
        }
    }
}
