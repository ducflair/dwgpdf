use dwgpdf::convert_file_to_pdf;
use dwgpdf::options::ConversionOptions;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: convert_one <in_file> <out_pdf>");
        return;
    }
    let in_file = Path::new(&args[1]);
    let out_file = Path::new(&args[2]);
    let opts = ConversionOptions::default();
    let pdf = convert_file_to_pdf(in_file, &opts).expect("convert failed");
    fs::write(out_file, pdf).expect("write failed");
    println!("Successfully converted {} to {}", in_file.display(), out_file.display());
}
