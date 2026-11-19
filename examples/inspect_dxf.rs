use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "tests/fixtures/dxf/smoke-plain.dxf".to_string());
    let bytes = fs::read(&path).unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    let doc_bounds = dwgpdf::cad::bounds::calculate_document_bounds(&doc, true);
    println!(
        "Document bounds: ({:.2},{:.2}) to ({:.2},{:.2})",
        doc_bounds.min_x, doc_bounds.min_y, doc_bounds.max_x, doc_bounds.max_y
    );
    println!(
        "Doc bounds aspect: {:.4}",
        (doc_bounds.max_x - doc_bounds.min_x) / (doc_bounds.max_y - doc_bounds.min_y)
    );
    for be in doc.entities_in_block("PROJECTOR HORN") {
        let mut b = dwgpdf::cad::bounds::BoundingBox::new();
        let mut visited = std::collections::HashSet::new();
        dwgpdf::cad::bounds::accumulate_entity_bounds(
            &doc,
            be,
            &dwgpdf::cad::bounds::Transform2D::identity(),
            &mut b,
            &mut visited,
            0,
            None,
        );
        println!(
            "  PROJECTOR HORN ent: {:?}, layer={}, bbox=({:.1},{:.1}) to ({:.1},{:.1})",
            std::mem::discriminant(be),
            be.as_entity().layer(),
            b.min_x,
            b.min_y,
            b.max_x,
            b.max_y
        );
        if let EntityType::MText(mt) = be {
            println!(
                "    MTEXT: '{}', pt=({:.1},{:.1})",
                mt.value, mt.insertion_point.x, mt.insertion_point.y
            );
        }
    }

    println!("Layers count: {}", doc.layers.len());
    for l in doc.layers.iter() {
        println!(
            "  Layer '{}': is_off={}, is_frozen={}",
            l.name,
            l.is_off(),
            l.is_frozen()
        );
    }
    for (_i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
        let mut b = dwgpdf::cad::bounds::BoundingBox::new();
        let mut visited = std::collections::HashSet::new();
        dwgpdf::cad::bounds::accumulate_entity_bounds(
            &doc,
            ent,
            &dwgpdf::cad::bounds::Transform2D::identity(),
            &mut b,
            &mut visited,
            0,
            None,
        );
        for (i, ent) in doc.entities_in_block("*Model_Space").enumerate() {
            if let EntityType::Insert(ins) = ent {
                for att in &ins.attributes {
                    println!(
                        "INSERT [{}] block='{}' ATTR: tag='{}', val='{}', layer='{}', invis={}",
                        i,
                        ins.block_name,
                        att.tag,
                        att.value,
                        att.common.layer,
                        att.flags.invisible
                    );
                }
            }
        }
    }
}
