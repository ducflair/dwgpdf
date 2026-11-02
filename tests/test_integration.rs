mod common;

use common::{
    ensure_fixtures, get_all_dwf_fixtures, get_all_dwg_fixtures, get_all_dxf_fixtures,
    save_test_output, validate_pdf, OFFICIAL_DWF_FILES, OFFICIAL_DWG_FILES, OFFICIAL_DXF_FILES,
};
use dwgpdf::{
    convert_dwf_to_pdf, convert_dwg_to_pdf, convert_dxf_to_pdf, convert_file_to_pdf,
    convert_to_pdf, ColorMode, ConversionMode, ConversionOptions, Metadata, Orientation, PageSize,
};
use std::fs;
use std::path::Path;

#[test]
fn test_all_dxf_fixtures() {
    ensure_fixtures();
    let dxf_files = get_all_dxf_fixtures();

    println!("Found {} DXF fixture files to test", dxf_files.len());
    assert!(
        dxf_files.len() >= OFFICIAL_DXF_FILES.len(),
        "Expected at least {} DXF fixtures, found {}",
        OFFICIAL_DXF_FILES.len(),
        dxf_files.len()
    );

    let opts = ConversionOptions::default();

    for path in &dxf_files {
        let name = path.file_name().unwrap().to_str().unwrap();
        println!("  Testing DXF: {}", name);

        let pdf = convert_file_to_pdf(path, &opts)
            .unwrap_or_else(|e| panic!("Failed to convert DXF file {}: {:?}", name, e));

        validate_pdf(&pdf);

        let out_name = format!("dxf/{}.pdf", path.file_stem().unwrap().to_str().unwrap());
        save_test_output(&out_name, &pdf);
    }
}

#[test]
fn test_all_dwg_fixtures() {
    ensure_fixtures();
    let dwg_files = get_all_dwg_fixtures();

    println!("Found {} DWG fixture files to test", dwg_files.len());
    assert!(
        dwg_files.len() >= OFFICIAL_DWG_FILES.len(),
        "Expected at least {} DWG fixtures, found {}",
        OFFICIAL_DWG_FILES.len(),
        dwg_files.len()
    );

    let opts = ConversionOptions::default();

    for path in &dwg_files {
        let name = path.file_name().unwrap().to_str().unwrap();
        println!("  Testing DWG: {}", name);

        let pdf = convert_file_to_pdf(path, &opts)
            .unwrap_or_else(|e| panic!("Failed to convert DWG file {}: {:?}", name, e));

        validate_pdf(&pdf);

        let out_name = format!("dwg/{}.pdf", path.file_stem().unwrap().to_str().unwrap());
        save_test_output(&out_name, &pdf);
    }
}

#[test]
fn test_all_dwf_fixtures() {
    ensure_fixtures();
    let dwf_files = get_all_dwf_fixtures();

    println!("Found {} DWF fixture files to test", dwf_files.len());
    assert!(
        dwf_files.len() >= OFFICIAL_DWF_FILES.len(),
        "Expected at least {} DWF fixtures, found {}",
        OFFICIAL_DWF_FILES.len(),
        dwf_files.len()
    );

    let opts = ConversionOptions::default();

    for path in &dwf_files {
        let name = path.file_name().unwrap().to_str().unwrap();
        println!("  Testing DWF: {}", name);

        let pdf = convert_file_to_pdf(path, &opts)
            .unwrap_or_else(|e| panic!("Failed to convert DWF file {}: {:?}", name, e));

        validate_pdf(&pdf);

        let out_name = format!("dwf/{}.pdf", path.file_stem().unwrap().to_str().unwrap());
        save_test_output(&out_name, &pdf);
    }
}

#[test]
fn test_direct_memory_conversions() {
    ensure_fixtures();
    let dxf_files = get_all_dxf_fixtures();
    let dwg_files = get_all_dwg_fixtures();
    let dwf_files = get_all_dwf_fixtures();

    let opts = ConversionOptions::default();

    // Direct DXF memory conversion
    if let Some(first_dxf) = dxf_files.first() {
        let bytes = fs::read(first_dxf).expect("read dxf bytes");
        let pdf = convert_dxf_to_pdf(&bytes, &opts).expect("convert_dxf_to_pdf must succeed");
        validate_pdf(&pdf);
    }

    // Direct DWG memory conversion
    if let Some(first_dwg) = dwg_files.first() {
        let bytes = fs::read(first_dwg).expect("read dwg bytes");
        let pdf = convert_dwg_to_pdf(&bytes, &opts).expect("convert_dwg_to_pdf must succeed");
        validate_pdf(&pdf);
    }

    // Direct DWF memory conversion
    if let Some(first_dwf) = dwf_files.first() {
        let bytes = fs::read(first_dwf).expect("read dwf bytes");
        let pdf = convert_dwf_to_pdf(&bytes, &opts).expect("convert_dwf_to_pdf must succeed");
        validate_pdf(&pdf);
    }
}

#[test]
fn test_options_color_modes() {
    ensure_fixtures();
    let dxf_files = get_all_dxf_fixtures();
    let sample = dxf_files
        .iter()
        .find(|p| p.file_name().unwrap() == "columns_R2007.dxf")
        .or_else(|| dxf_files.first())
        .expect("columns_R2007.dxf or another DXF fixture required");

    let bytes = fs::read(sample).expect("read dxf bytes");

    let modes = [
        ColorMode::Normal,
        ColorMode::Monochrome,
        ColorMode::Grayscale,
        ColorMode::Invert,
    ];

    for mode in modes {
        let mut opts = ConversionOptions::default();
        opts.color_mode = mode;
        let pdf = convert_dxf_to_pdf(&bytes, &opts)
            .unwrap_or_else(|e| panic!("convert dxf with color mode {:?}: {:?}", mode, e));
        validate_pdf(&pdf);
    }
}

#[test]
fn test_options_page_sizes_and_orientations() {
    ensure_fixtures();
    let dxf_files = get_all_dxf_fixtures();
    let sample = dxf_files
        .iter()
        .find(|p| p.file_name().unwrap() == "columns_R2007.dxf")
        .or_else(|| dxf_files.first())
        .expect("DXF fixture required");

    let bytes = fs::read(sample).expect("read dxf bytes");

    let page_sizes = [
        PageSize::Auto,
        PageSize::A4,
        PageSize::A3,
        PageSize::Letter,
        PageSize::Custom(400.0, 400.0),
    ];

    let orientations = [
        Orientation::Auto,
        Orientation::Portrait,
        Orientation::Landscape,
    ];

    for page_size in page_sizes {
        for orientation in &orientations {
            let mut opts = ConversionOptions::default();
            opts.page_size = page_size;
            opts.orientation = *orientation;

            let pdf = convert_dxf_to_pdf(&bytes, &opts)
                .unwrap_or_else(|e| panic!("convert dxf with {:?}/{:?}: {:?}", page_size, orientation, e));
            validate_pdf(&pdf);
        }
    }
}

#[test]
fn test_options_crop_mode() {
    ensure_fixtures();
    let dxf_files = get_all_dxf_fixtures();
    let sample = dxf_files
        .iter()
        .find(|p| p.file_name().unwrap() == "columns_R2007.dxf")
        .or_else(|| dxf_files.first())
        .expect("DXF fixture required");

    let bytes = fs::read(sample).expect("read dxf bytes");

    let mut opts = ConversionOptions::default();
    opts.mode = ConversionMode::Crop {
        offset_x: 10.0,
        offset_y: 10.0,
        width: Some(100.0),
        height: Some(100.0),
    };

    let pdf = convert_dxf_to_pdf(&bytes, &opts).expect("crop mode conversion must succeed");
    validate_pdf(&pdf);
    save_test_output("columns_R2007_cropped.pdf", &pdf);
}

#[test]
fn test_options_metadata_and_ocg_layers() {
    ensure_fixtures();
    let dxf_files = get_all_dxf_fixtures();
    let sample = dxf_files
        .iter()
        .find(|p| p.file_name().unwrap() == "columns_R2007.dxf")
        .or_else(|| dxf_files.first())
        .expect("DXF fixture required");

    let bytes = fs::read(sample).expect("read dxf bytes");

    let mut opts = ConversionOptions::default();
    opts.export_layers = true;
    opts.metadata = Metadata {
        title: Some("Integration Test Title".to_string()),
        author: Some("Ducflair Tester".to_string()),
        subject: Some("CAD to PDF".to_string()),
        creator: Some("dwgpdf test".to_string()),
    };

    let pdf = convert_dxf_to_pdf(&bytes, &opts).expect("conversion with layers and metadata");
    validate_pdf(&pdf);

    // Verify metadata was stored in document Info dictionary
    let doc = hipdf::lopdf::Document::load_mem(&pdf).expect("parse generated pdf");
    let info_ref = doc.trailer.get(b"Info").expect("trailer must contain Info");
    let info_obj = doc.get_object(info_ref.as_reference().unwrap()).unwrap();
    let info_dict = info_obj.as_dict().unwrap();

    let title_obj = info_dict.get(b"Title").unwrap();
    assert_eq!(title_obj.as_str().unwrap(), b"Integration Test Title");

    let author_obj = info_dict.get(b"Author").unwrap();
    assert_eq!(author_obj.as_str().unwrap(), b"Ducflair Tester");
}

#[test]
fn test_error_handling() {
    // 1. Invalid bytes
    let invalid_bytes = b"not a cad file header";
    let res = convert_to_pdf(invalid_bytes, &ConversionOptions::default());
    assert!(res.is_err(), "Invalid bytes should return an error");

    // 2. Non-existent file
    let res = convert_file_to_pdf(Path::new("non_existent_file.dxf"), &ConversionOptions::default());
    assert!(res.is_err(), "Non-existent file should return an error");
}

#[test]
fn test_multi_page_and_last_page_model() {
    let local_dir = Path::new("tests/fixtures/local");
    if local_dir.exists() {
        // 1. Files with 1 enabled paper layout + 1 model space = 2 pages
        for file_name in &["HG8-GTC-EXE-001-R02.dwg", "HG5-ACV-EXE-001-R01%20_%20Telas%20Finais.dwg"] {
            let file_path = local_dir.join(file_name);
            if file_path.exists() {
                let opts = ConversionOptions::default();
                let pdf_bytes = convert_file_to_pdf(&file_path, &opts)
                    .unwrap_or_else(|e| panic!("Failed to convert {}: {:?}", file_name, e));
                validate_pdf(&pdf_bytes);

                let doc = hipdf::lopdf::Document::load_mem(&pdf_bytes).expect("parse generated pdf");
                let pages = doc.get_pages();
                assert_eq!(pages.len(), 2, "Expected exactly 2 pages (layout + model) for {}", file_name);
            }
        }

        // 2. File with Layout1 (enabled), Layout2 (empty/hidden), and Model space:
        // Must exclude Layout2 and produce exactly 2 pages (Layout1 + Model space).
        let hg_pr = local_dir.join("Honest%20Green%20Principe%20Real (1).dwg");
        if hg_pr.exists() {
            let opts = ConversionOptions::default();
            let pdf_bytes = convert_file_to_pdf(&hg_pr, &opts)
                .unwrap_or_else(|e| panic!("Failed to convert Honest Green DWG: {:?}", e));
            validate_pdf(&pdf_bytes);

            let doc = hipdf::lopdf::Document::load_mem(&pdf_bytes).expect("parse generated pdf");
            let pages = doc.get_pages();
            assert_eq!(pages.len(), 2, "Expected exactly 2 pages (Layout1 + Model, excluding empty Layout2)");
        }

        // 3. File with no populated layouts (Sheetmetal Jeep has unused empty Layout1):
        // Must exclude unused Layout1 and produce exactly 1 page (Model space).
        let jeep = local_dir.join("Sheetmetal Jeep.dxf");
        if jeep.exists() {
            let opts = ConversionOptions::default();
            let pdf_bytes = convert_file_to_pdf(&jeep, &opts)
                .unwrap_or_else(|e| panic!("Failed to convert Sheetmetal Jeep DXF: {:?}", e));
            validate_pdf(&pdf_bytes);

            let doc = hipdf::lopdf::Document::load_mem(&pdf_bytes).expect("parse generated pdf");
            let pages = doc.get_pages();
            assert_eq!(pages.len(), 1, "Expected exactly 1 page (Model space, excluding empty Layout1)");
        }

        // 4. Multi-sheet architectural file (HG8_ARQ has 50 active layouts + 1 model space):
        // Must include all 50 active layouts ordered by tab_order, with model space as page 51.
        let hg8 = local_dir.join("HG8_ARQ.dwg");
        if hg8.exists() {
            let opts = ConversionOptions::default();
            let pdf_bytes = convert_file_to_pdf(&hg8, &opts)
                .unwrap_or_else(|e| panic!("Failed to convert HG8_ARQ.dwg: {:?}", e));
            validate_pdf(&pdf_bytes);

            let doc = hipdf::lopdf::Document::load_mem(&pdf_bytes).expect("parse generated pdf");
            let pages = doc.get_pages();
            assert_eq!(pages.len(), 52, "Expected exactly 52 pages (51 layouts in tab order + 1 model space)");
        }
    }
}

#[test]
fn test_lineweights_toggle() {
    let test_file = Path::new("tests/fixtures/dxf/wipeout_door.dxf");
    if !test_file.exists() {
        return;
    }

    // 1. Default (use_lineweights = false): uniform thin hairline (0 w)
    let opts_default = ConversionOptions::default();
    assert!(!opts_default.use_lineweights);
    assert_eq!(opts_default.min_line_width_pt, 0.0);
    let pdf_default = convert_file_to_pdf(test_file, &opts_default).expect("convert default");
    validate_pdf(&pdf_default);

    let doc_default = hipdf::lopdf::Document::load_mem(&pdf_default).expect("parse pdf");
    let mut default_widths = std::collections::HashSet::new();
    for (_, page_id) in doc_default.get_pages() {
        if let Ok(content_data) = doc_default.get_page_content(page_id) {
            let content_str = String::from_utf8_lossy(&content_data);
            for line in content_str.lines() {
                if let Some(pos) = line.rfind(" w") {
                    if pos + 2 == line.len() {
                        if let Some(num_str) = line[..pos].split_whitespace().last() {
                            default_widths.insert(num_str.to_string());
                        }
                    }
                }
            }
        }
    }
    // Under uniform thin hairline mode, all lines have uniform width (0 w)
    assert!(
        default_widths.contains("0") || default_widths.contains("0.00") || default_widths.len() <= 1,
        "Expected 0 in default widths: {:?}",
        default_widths
    );

    // 2. Opt-in (use_lineweights = true): CAD lineweights enabled
    let opts_lineweights = ConversionOptions {
        use_lineweights: true,
        ..Default::default()
    };
    let pdf_lineweights = convert_file_to_pdf(test_file, &opts_lineweights).expect("convert with lineweights");
    validate_pdf(&pdf_lineweights);
}


