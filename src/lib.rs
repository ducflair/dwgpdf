pub mod cad;
pub mod converter;
pub mod dwf;
pub mod error;
pub mod options;
pub mod pdf;

pub use converter::{
    convert_dwf_to_pdf, convert_dwg_to_pdf, convert_dxf_to_pdf, convert_file_to_pdf, convert_to_pdf,
};
pub use error::{ConversionError, ConversionResult};
pub use options::{
    ColorMode, ConversionMode, ConversionOptions, Metadata, Orientation, PageSize,
};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
