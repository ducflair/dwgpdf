use dwgpdf::cad::mtext::parse_mtext_details;

fn main() {
    let raw = r"\pi48.38063;OPEN-SOURCE \P\pi28.61255;ARCHITECTURAL\P\pi55.90587;DRAWING SET";
    let details = parse_mtext_details(raw, 384.0, 18.0);
    println!("Details for OPEN-SOURCE:");
    for (i, l) in details.lines.iter().enumerate() {
        println!("  Line {}: text='{}' align={:?} indent={} tf={} wf={} spans={}",
            i, l.text, l.alignment, l.indent, l.tracking_factor, l.width_factor, l.spans.len());
    }

    let cc_raw = r"{\fArial|b1|i0|c0|p34;cc}";
    let cc_details = parse_mtext_details(cc_raw, 6.00095, 38.8125);
    println!("Details for cc:");
    for (i, l) in cc_details.lines.iter().enumerate() {
        println!("  Line {}: text='{}' align={:?} indent={} tf={} wf={} spans={}",
            i, l.text, l.alignment, l.indent, l.tracking_factor, l.width_factor, l.spans.len());
    }
}
