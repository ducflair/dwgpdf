pub mod builder;
pub mod geometry;

use crate::error::ConversionResult;
use crate::options::ConversionOptions;
use acadrust::document::CadDocument;
use builder::CadToPdfBuilder;

/// Converts a CadDocument (DWG or DXF) to PDF bytes using hipdf.
pub fn convert_cad_to_pdf(doc: &CadDocument, options: ConversionOptions) -> ConversionResult<Vec<u8>> {
    let builder = CadToPdfBuilder::new(doc, options)?;
    builder.build()
}
