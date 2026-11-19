use crate::{convert_to_pdf, ConversionOptions};
use wasm_bindgen::prelude::*;

/// Convert CAD bytes to PDF using default conversion options.
#[wasm_bindgen(js_name = convert)]
pub fn convert_bytes(input: &[u8]) -> Result<Vec<u8>, JsValue> {
    convert_to_pdf(input, &ConversionOptions::default()).map_err(to_js_error)
}

/// Convert CAD bytes to PDF using a JavaScript `ConversionOptions` object.
#[wasm_bindgen(js_name = convertWithOptions)]
pub fn convert_bytes_with_options(input: &[u8], options: JsValue) -> Result<Vec<u8>, JsValue> {
    let options =
        serde_wasm_bindgen::from_value::<ConversionOptions>(options).map_err(to_js_error)?;
    convert_to_pdf(input, &options).map_err(to_js_error)
}

fn to_js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
