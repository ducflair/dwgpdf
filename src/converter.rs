use crate::cad::{detect_format, load_cad_from_bytes, CadFormat};
use crate::dwf::convert_dwf_to_pdf as dwf_to_pdf;
use crate::error::{ConversionError, ConversionResult};
use crate::options::ConversionOptions;
use crate::pdf::convert_cad_to_pdf;
use std::path::Path;

/// High-level function to convert any supported CAD format (DXF, DWG, DWF) to PDF.
pub fn convert_to_pdf(input_bytes: &[u8], options: &ConversionOptions) -> ConversionResult<Vec<u8>> {
    let format = detect_format(input_bytes);
    match format {
        CadFormat::Dwf => dwf_to_pdf(input_bytes, options),
        CadFormat::Dwg | CadFormat::Dxf | CadFormat::Unknown => {
            let doc = load_cad_from_bytes(input_bytes)?;
            convert_cad_to_pdf(&doc, options.clone())
        }
    }
}

/// Convert DXF bytes directly to PDF.
pub fn convert_dxf_to_pdf(dxf_bytes: &[u8], options: &ConversionOptions) -> ConversionResult<Vec<u8>> {
    let doc = load_cad_from_bytes(dxf_bytes)?;
    convert_cad_to_pdf(&doc, options.clone())
}

/// Convert DWG bytes directly to PDF.
pub fn convert_dwg_to_pdf(dwg_bytes: &[u8], options: &ConversionOptions) -> ConversionResult<Vec<u8>> {
    let doc = load_cad_from_bytes(dwg_bytes)?;
    convert_cad_to_pdf(&doc, options.clone())
}

/// Convert DWF bytes directly to PDF.
pub fn convert_dwf_to_pdf(dwf_bytes: &[u8], options: &ConversionOptions) -> ConversionResult<Vec<u8>> {
    dwf_to_pdf(dwf_bytes, options)
}

/// Convert CAD file at path to PDF bytes.
pub fn convert_file_to_pdf(input_path: &Path, options: &ConversionOptions) -> ConversionResult<Vec<u8>> {
    let bytes = std::fs::read(input_path).map_err(ConversionError::Io)?;
    convert_to_pdf(&bytes, options)
}
