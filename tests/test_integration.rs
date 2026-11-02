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
