use dwgpdf::cad::load_cad_from_bytes;

fn main() {
    let bytes = std::fs::read("tests/fixtures/dxf/wipeout_door.dxf").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();
    let br = doc.block_records.get("*Model_Space").unwrap();
    let st = doc.objects.values().find_map(|obj| {
        if let acadrust::objects::ObjectType::SortEntitiesTable(ref st) = obj {
            if st.block_owner_handle == br.handle {
                return Some(st);
            }
        }
        None
    }).unwrap();

    let mut entities: Vec<_> = br.entity_handles
        .iter()
        .filter_map(|h| doc.get_entity(*h))
        .collect();

    println!("Entities before sort: {}", entities.len());
    for (i, e) in entities.iter().enumerate() {
        let h = e.common().handle;
        let s = st.get_sort_handle(h).unwrap_or(h);
        println!("[{}] handle={:?} sort={:?} layer={}", i, h, s, e.common().layer);
    }

    entities.sort_by_key(|e| {
        let h = e.common().handle;
        st.get_sort_handle(h).unwrap_or(h).value()
    });

    println!("\nEntities AFTER sort (first 10):");
    for (i, e) in entities.iter().take(10).enumerate() {
        let h = e.common().handle;
        let s = st.get_sort_handle(h).unwrap_or(h);
        println!("[{}] handle={:?} sort={:?} layer={} type={:?}", i, h, s, e.common().layer, match e {
            acadrust::entities::EntityType::LwPolyline(lw) => format!("LwPolyline (pts={})", lw.vertices.len()),
            acadrust::entities::EntityType::Insert(ins) => format!("Insert ({})", ins.block_name),
            acadrust::entities::EntityType::MText(mt) => format!("MText ({})", mt.value),
            acadrust::entities::EntityType::Line(_) => "Line".to_string(),
            acadrust::entities::EntityType::Arc(_) => "Arc".to_string(),
            acadrust::entities::EntityType::Circle(_) => "Circle".to_string(),
            acadrust::entities::EntityType::Wipeout(_) => "Wipeout".to_string(),
            _ => "Other".to_string(),
        });
    }
}
