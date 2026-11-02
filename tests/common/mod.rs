use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

static INIT_FIXTURES: Once = Once::new();

pub const FIXTURE_BASE_URL: &str = "https://raw.githubusercontent.com/ducflair/fixtures/main/src";

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

pub const OFFICIAL_DWF_FILES: &[&str] = &[
    "FlyAshSystem.dwf",
];

pub fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn tests_output_dir() -> PathBuf {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tests_output");
    let _ = fs::create_dir_all(&out);
    out
}

pub fn save_test_output(name: &str, pdf_bytes: &[u8]) {
    let out_path = tests_output_dir().join(name);
    if let Some(parent) = out_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(out_path, pdf_bytes);
}

/// Ensures all official fixtures are available in `tests/fixtures/`.
/// If any file is missing, it tries copying from a local checkout or downloads via curl.
/// Local user-provided files are never touched or modified.
pub fn ensure_fixtures() {
    INIT_FIXTURES.call_once(|| {
        let root = fixtures_root();
        let _ = fs::create_dir_all(root.join("dxf"));
        let _ = fs::create_dir_all(root.join("dwg"));
        let _ = fs::create_dir_all(root.join("dwf"));
        let _ = fs::create_dir_all(root.join("local"));

        let local_src_candidates = [
            std::env::var("FIXTURES_DIR").ok().map(PathBuf::from),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().map(|p| p.join("fixtures/src")),
        ];

        let sync_group = |files: &[&str], subfolder: &str, remote_folder: &str| {
            let target_dir = root.join(subfolder);
            for file in files {
                let target_path = target_dir.join(file);
                if target_path.exists() && fs::metadata(&target_path).map(|m| m.len()).unwrap_or(0) > 0 {
                    continue; // Already present, leave untouched
                }

                // 1. Try local mirror first
                let mut copied = false;
                for cand in local_src_candidates.iter().flatten() {
                    let src_file = cand.join(remote_folder).join(file);
                    if src_file.exists() && fs::metadata(&src_file).map(|m| m.len()).unwrap_or(0) > 0 {
                        if fs::copy(&src_file, &target_path).is_ok() {
                            copied = true;
                            break;
                        }
                    }
                }

                // 2. Fall back to downloading from GitHub
                if !copied {
                    let url = format!("{}/{}/{}", FIXTURE_BASE_URL, remote_folder, file);
                    let status = Command::new("curl")
                        .args(["-fsSL", &url, "-o", target_path.to_str().unwrap()])
                        .status();
                    if let Ok(st) = status {
                        if !st.success() {
                            eprintln!("Warning: Failed to fetch fixture from {}", url);
                        }
                    }
                }
            }
        };

        sync_group(OFFICIAL_DXF_FILES, "dxf", "dxf-files");
        sync_group(OFFICIAL_DWG_FILES, "dwg", "dwg-files");
        sync_group(OFFICIAL_DWF_FILES, "dwf", "dwf-files");
    });
}

/// Recursively finds all CAD files matching a specific extension.
/// Scans both the standard type directory and the `local` directory.
pub fn find_fixtures_with_ext(ext: &str) -> Vec<PathBuf> {
    ensure_fixtures();
    let root = fixtures_root();
    let mut results = Vec::new();

    let scan_dir = |dir: &Path, results: &mut Vec<PathBuf>| {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(file_ext) = path.extension() {
                        if file_ext.eq_ignore_ascii_case(ext) {
                            results.push(path);
                        }
                    }
                }
            }
        }
    };

    let target_sub = match ext.to_lowercase().as_str() {
        "dxf" => "dxf",
        "dwg" => "dwg",
        "dwf" => "dwf",
        _ => "",
    };

    if !target_sub.is_empty() {
        scan_dir(&root.join(target_sub), &mut results);
    }
    // Also scan `local` for user-added files
    scan_dir(&root.join("local"), &mut results);

    results.sort();
    results
}

pub fn get_all_dxf_fixtures() -> Vec<PathBuf> {
    find_fixtures_with_ext("dxf")
}

pub fn get_all_dwg_fixtures() -> Vec<PathBuf> {
    find_fixtures_with_ext("dwg")
}

pub fn get_all_dwf_fixtures() -> Vec<PathBuf> {
    find_fixtures_with_ext("dwf")
}

/// Validates that the generated PDF data is compliant:
/// 1. Has minimum length (> 100 bytes)
/// 2. Starts with standard '%PDF-' magic header
/// 3. Ends with '%%EOF' trailer marker
/// 4. Can be parsed back by lopdf into a valid Document structure
pub fn validate_pdf(data: &[u8]) {
    assert!(data.len() > 100, "PDF byte length too small: {} bytes", data.len());

    let header = std::str::from_utf8(&data[..8.min(data.len())]).unwrap_or("");
    assert!(header.starts_with("%PDF-"), "Invalid PDF header: {}", header);

    let tail_len = 32.min(data.len());
    let tail_slice = &data[data.len() - tail_len..];
    let tail_str = String::from_utf8_lossy(tail_slice);
    assert!(
        tail_str.contains("%%EOF"),
        "PDF trailer missing %%EOF marker: {}",
        tail_str
    );

    // Validate that lopdf can parse the document structure
    let doc = hipdf::lopdf::Document::load_mem(data)
        .expect("Generated PDF must be parseable by lopdf");
    assert!(!doc.objects.is_empty(), "PDF must contain objects");

    // Verify Catalog exists
    let catalog = doc.catalog().expect("PDF must contain a valid Catalog dictionary");
    assert!(
        catalog.get(b"Pages").is_ok(),
        "PDF Catalog must reference Pages"
    );
}
