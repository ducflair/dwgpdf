use dwgpdf::cad::load_cad_from_bytes;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/sample_AC1014.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    println!("DimStyles in sample_AC1014.dwg:");
    for ds in doc.dim_styles.iter() {
        println!("  '{}': dimtxt={}, dimgap={}, dimasz={}, dimscale={}",
            ds.name, ds.dimtxt, ds.dimgap, ds.dimasz, ds.dimscale);
    }

    println!("\nTolerance entities in sample_AC1014.dwg:");
    for (i, e) in doc.entities().enumerate() {
        if let acadrust::entities::EntityType::Tolerance(t) = e {
            println!("  #{}: style='{}', txt_h={}, gap={}, ins=({},{}), dir=({},{}), starts_quote={}, text={:?}",
                i, t.dimension_style_name, t.text_height, t.dimension_gap,
                t.insertion_point.x, t.insertion_point.y,
                t.direction.x, t.direction.y,
                t.text.starts_with('"'),
                t.text
            );
        }
    }
}
