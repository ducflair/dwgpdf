use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/example_2000.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("\nBlock 'bloko' entities:");
    for ent in doc.entities_in_block("bloko") {
        println!("  {:?}", ent);
    }
    println!("\nInserts of 'bloko':");
    for ent in doc.entities_in_block("*Model_Space") {
        if let EntityType::Insert(ins) = ent {
            if ins.block_name == "bloko" {
                println!("  Insert 'bloko': pt=({},{}), scale=({},{},{}), rot={}, attribs={:?}",
                    ins.insert_point.x, ins.insert_point.y, ins.x_scale(), ins.y_scale(), ins.z_scale(), ins.rotation,
                    ins.attributes);
            }
        }
    }
}
