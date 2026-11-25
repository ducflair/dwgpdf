use std::fs;
use std::path::{Path, PathBuf};

pub const OFFICIAL_DXF_FILES: &[&str] = &[
    "columns_R2007.dxf",
    "hatches_2.dxf",
    "house-plan.dxf",
    "mtext_complex_inline_codes.dxf",
    "text.dxf",
    "text_fonts.dxf",
    "text_mirror_shx_font.dxf",
    "text_mirror_true_type_font.dxf",
    "text_negative_extrusion.dxf",
    "uncommon.dxf",
    "wipeout_door.dxf",
];

pub const OFFICIAL_DWG_FILES: &[&str] = &[
    "Electrical-Plan-Legend.dwg",
    "People-and-Wheelchairs-Dimensions.dwg",
    "sample_AC1014.dwg",
    "sample_AC1015.dwg",
    "sample_AC1018.dwg",
    "sample_AC1021.dwg",
    "sample_AC1024.dwg",
    "sample_AC1027.dwg",
    "sample_AC1032.dwg",
];

pub const OFFICIAL_DWF_FILES: &[&str] = &["FlyAshSystem.dwf"];

pub fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn tests_output_dir() -> PathBuf {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/output");
    let _ = fs::create_dir_all(&out);
    out
}

pub fn save_test_output(name: &str, pdf_bytes: &[u8]) {
    let out_path = tests_output_dir().join(name);
    if let Some(parent) = out_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(e) = fs::write(&out_path, pdf_bytes) {
        eprintln!(
            "Warning: failed to write test output {}: {:?}",
            out_path.display(),
            e
        );
    }
}

/// Returns only the fixed official fixtures used by the repeatable integration suite.
fn official_fixtures_with_ext(ext: &str) -> Vec<PathBuf> {
    let root = fixtures_root();
    let (subdirectory, files) = match ext.to_ascii_lowercase().as_str() {
        "dxf" => ("dxf", OFFICIAL_DXF_FILES),
        "dwg" => ("dwg", OFFICIAL_DWG_FILES),
        "dwf" => ("dwf", OFFICIAL_DWF_FILES),
        _ => return Vec::new(),
    };

    files
        .iter()
        .map(|name| root.join(subdirectory).join(name))
        .collect()
}

/// Finds fixtures in a caller-provided `local/` directory, returning empty if none are present.
pub fn find_local_fixtures_with_ext(root: &Path, ext: &str) -> Vec<PathBuf> {
    let local_dir = root.join("local");
    let mut results = fs::read_dir(local_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.extension()
                .is_some_and(|file_ext| file_ext.eq_ignore_ascii_case(ext))
        })
        .collect::<Vec<_>>();
    results.sort();
    results
}

pub fn get_all_dxf_fixtures() -> Vec<PathBuf> {
    official_fixtures_with_ext("dxf")
}

pub fn get_all_dwg_fixtures() -> Vec<PathBuf> {
    official_fixtures_with_ext("dwg")
}

pub fn get_all_dwf_fixtures() -> Vec<PathBuf> {
    official_fixtures_with_ext("dwf")
}

/// Validates that the generated PDF data is compliant:
/// 1. Has minimum length (> 100 bytes)
/// 2. Starts with standard '%PDF-' magic header
/// 3. Ends with '%%EOF' trailer marker
/// 4. Can be parsed back by lopdf into a valid Document structure
pub fn validate_pdf(data: &[u8]) {
    assert!(
        data.len() > 100,
        "PDF byte length too small: {} bytes",
        data.len()
    );

    let header = std::str::from_utf8(&data[..8.min(data.len())]).unwrap_or("");
    assert!(
        header.starts_with("%PDF-"),
        "Invalid PDF header: {}",
        header
    );

    let tail_len = 32.min(data.len());
    let tail_slice = &data[data.len() - tail_len..];
    let tail_str = String::from_utf8_lossy(tail_slice);
    assert!(
        tail_str.contains("%%EOF"),
        "PDF trailer missing %%EOF marker: {}",
        tail_str
    );

    // Validate that lopdf can parse the document structure
    let doc =
        hipdf::lopdf::Document::load_mem(data).expect("Generated PDF must be parseable by lopdf");
    assert!(!doc.objects.is_empty(), "PDF must contain objects");

    // Verify Catalog exists
    let catalog = doc
        .catalog()
        .expect("PDF must contain a valid Catalog dictionary");
    assert!(
        catalog.get(b"Pages").is_ok(),
        "PDF Catalog must reference Pages"
    );
}
