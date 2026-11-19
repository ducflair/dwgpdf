use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    let target_blocks = ["*X"];
    for bname in &target_blocks {
        println!("\n=== Block '{}' ===", bname);
        for ent in doc.entities_in_block(bname) {
            match ent {
                EntityType::Text(t) => println!("  Text: '{}', layer='{}', color={:?}", t.value, t.common.layer, t.common.color),
                EntityType::MText(mt) => println!("  MText: '{}', layer='{}', color={:?}", mt.value, mt.common.layer, mt.common.color),
                EntityType::AttributeDefinition(att) => println!("  AttDef: tag='{}', def='{}', layer='{}', invis={}", att.tag, att.default_value, att.common.layer, att.flags.invisible),
                EntityType::Insert(ins) => println!("  Insert: block='{}', layer='{}', attribs={:?}", ins.block_name, ins.common.layer, ins.attributes.iter().map(|a| (&a.tag, &a.value)).collect::<Vec<_>>()),
                other => println!("  Other: {:?}, layer='{}'", std::mem::discriminant(other), other.common().layer),
            }
        }
    }
}
