use acadrust::entities::EntityType;
use dwgpdf::cad::load_cad_from_bytes;
use std::collections::HashMap;
use std::fs;

fn main() {
    let bytes = fs::read("tests/fixtures/dwg/Electrical-Plan-Legend.dwg").unwrap();
    let doc = load_cad_from_bytes(&bytes).unwrap();

    let mut layer_legend_counts: HashMap<String, usize> = HashMap::new();
    let mut layer_outside_counts: HashMap<String, usize> = HashMap::new();

    for ent in doc.entities_in_block("*Model_Space") {
        let layer = ent.common().layer.clone();
        // check if entity is in legend area (x: 0..11000, y: -36000..100)
        let pt = match ent {
            EntityType::Insert(ins) => (ins.insert_point.x, ins.insert_point.y),
            EntityType::Text(t) => (t.insertion_point.x, t.insertion_point.y),
            EntityType::MText(mt) => (mt.insertion_point.x, mt.insertion_point.y),
            EntityType::Line(l) => (l.start.x, l.start.y),
            EntityType::Circle(c) => (c.center.x, c.center.y),
            EntityType::Arc(a) => (a.center.x, a.center.y),
            EntityType::LwPolyline(p) => (p.vertices.first().map(|v| v.location.x).unwrap_or(0.0), p.vertices.first().map(|v| v.location.y).unwrap_or(0.0)),
            _ => (0.0, 0.0),
        };
        let in_legend = pt.0 >= 0.0 && pt.0 <= 11000.0 && pt.1 >= -37000.0 && pt.1 <= 100.0;
        if in_legend {
            *layer_legend_counts.entry(layer).or_default() += 1;
        } else {
            *layer_outside_counts.entry(layer).or_default() += 1;
        }
    }

    println!("{:<35} | {:<12} | {:<12}", "Layer", "In Legend", "Outside");
    println!("{:-<35}-+-{:-<12}-+-{:-<12}", "", "", "");
    for l in doc.layers.iter() {
        let in_leg = layer_legend_counts.get(&l.name).copied().unwrap_or(0);
        let out_leg = layer_outside_counts.get(&l.name).copied().unwrap_or(0);
        println!("{:<35} | {:<12} | {:<12}", l.name, in_leg, out_leg);
    }
}
