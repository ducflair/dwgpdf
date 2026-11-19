use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    let blocks_to_check = [
        "PROJECTOR HORN",
        "STROBE",
        "OVEN",
        "FRIDGE PP",
        "AC POWER 2",
        "bollard light",
        "MODULE",
        "DUCT PROBE",
        "EWIS",
        "tv",
        "EM SPIT SURFACE",
        "PATCH PANEL",
        "small oyster",
    ];
    for name in &blocks_to_check {
        if let Some(br) = doc.block_records.iter().find(|b| b.name == *name) {
            println!(
                "Block '{}': base_point=({:.1}, {:.1}, {:.1})",
                br.name, br.base_point.x, br.base_point.y, br.base_point.z
            );
        }
        for ent in doc.entities_in_block("*Model_Space") {
            if let EntityType::Insert(ins) = ent {
                if ins.block_name == *name {
                    println!(
                        "  Inserted at ({:.1}, {:.1}) on layer '{}'",
                        ins.insert_point.x, ins.insert_point.y, ins.common.layer
                    );
                }
            }
        }
    }
}
