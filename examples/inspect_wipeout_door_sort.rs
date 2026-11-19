use dwgpdf::cad::load_cad_from_bytes;
use acadrust::objects::ObjectType;
use acadrust::entities::EntityType;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dxf/wipeout_door.dxf").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();
    let br_by_get = doc.block_records.get("*Model_Space");
    println!("br_by_get: {:?}", br_by_get.map(|b| (&b.name, b.handle)));
    println!("entities_in_block count: {}", doc.entities_in_block("*Model_Space").count());

    let st_opt = doc.objects.values().find_map(|obj| {
        if let ObjectType::SortEntitiesTable(ref st) = obj {
            Some(st)
        } else {
            None
        }
    });
    println!("st_opt: {:?}", st_opt.map(|s| (s.block_owner_handle, s.sorted_entries().len())));

    let br = br_by_get.unwrap();
    let st = st_opt.unwrap();
    let mut entities: Vec<&EntityType> = br.entity_handles
        .iter()
        .filter_map(|h| doc.get_entity(*h))
        .collect();

    println!("Total entities from br.entity_handles: {}", entities.len());

    entities.sort_by_key(|e| {
        let h = e.common().handle;
        st.get_sort_handle(h).unwrap_or(h).value()
    });

    let h226 = acadrust::Handle::from(0x226u64);
    let h423 = acadrust::Handle::from(0x423u64);
    for (idx, e) in entities.iter().enumerate() {
        let h = e.common().handle;
        let sort_h = st.get_sort_handle(h).unwrap_or(h);
        if h == h226 || h == h423 {
            println!("  [{}] handle {:?} (0x{:X}) -> sort {:?} (0x{:X})",
                idx, h, h.value(), sort_h, sort_h.value());
        }
    }
}
