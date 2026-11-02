use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PageSize {
    Auto,
    A0,
    A1,
    A2,
    A3,
    A4,
    A5,
    Letter,
    Legal,
    Tabloid,
    Custom(f64, f64), // (width_pt, height_pt)
}

impl Default for PageSize {
    fn default() -> Self {
        PageSize::Auto
    }
}

impl PageSize {
    /// Returns (width_pt, height_pt) in PDF points (1/72 inch).
    /// Standard ISO 216 paper sizes:
    /// A0: 841 x 1189 mm -> 2383.94 x 3370.39 pt
    /// A1: 594 x 841 mm  -> 1683.78 x 2383.94 pt
    /// A2: 420 x 594 mm  -> 1190.55 x 1683.78 pt
    /// A3: 297 x 420 mm  -> 841.89 x 1190.55 pt
    /// A4: 210 x 297 mm  -> 595.28 x 841.89 pt
    /// A5: 148 x 210 mm  -> 419.53 x 595.28 pt
    /// Letter: 8.5 x 11 in -> 612.0 x 792.0 pt
    /// Legal: 8.5 x 14 in  -> 612.0 x 1008.0 pt
    /// Tabloid: 11 x 17 in -> 792.0 x 1224.0 pt
    pub fn dimensions_pt(&self) -> Option<(f64, f64)> {
        match self {
            PageSize::Auto => None,
            PageSize::A0 => Some((2383.94, 3370.39)),
            PageSize::A1 => Some((1683.78, 2383.94)),
            PageSize::A2 => Some((1190.55, 1683.78)),
            PageSize::A3 => Some((841.89, 1190.55)),
            PageSize::A4 => Some((595.28, 841.89)),
            PageSize::A5 => Some((419.53, 595.28)),
            PageSize::Letter => Some((612.0, 792.0)),
            PageSize::Legal => Some((612.0, 1008.0)),
            PageSize::Tabloid => Some((792.0, 1224.0)),
            PageSize::Custom(w, h) => Some((*w, *h)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Orientation {
    #[default]
    Auto,
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorMode {
    #[default]
    Normal,
    Monochrome,
    Grayscale,
    Invert,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConversionMode {
    Plot,
    Crop {
        offset_x: f64,
        offset_y: f64,
        width: Option<f64>,
        height: Option<f64>,
    },
}

impl Default for ConversionMode {
    fn default() -> Self {
        ConversionMode::Plot
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionOptions {
    pub page_size: PageSize,
    pub orientation: Orientation,
    pub mode: ConversionMode,
    pub color_mode: ColorMode,
    pub margin_pt: f64,
    pub scale: Option<f64>,
    pub line_width_scale: f64,
    pub min_line_width_pt: f64,
    pub background_color: Option<String>,
    pub default_draw_color: Option<String>,
    pub export_layers: bool,
    pub layout: Option<String>,
    pub metadata: Metadata,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            page_size: PageSize::Auto,
            orientation: Orientation::Auto,
            mode: ConversionMode::Plot,
            color_mode: ColorMode::Normal,
            margin_pt: 20.0, // 20pt margin
            scale: None,
            line_width_scale: 1.0,
            min_line_width_pt: 0.5,
            background_color: Some("#ffffff".to_string()),
            default_draw_color: Some("#000000".to_string()),
            export_layers: true,
            layout: None,
            metadata: Metadata {
                creator: Some("dwgpdf (powered by hipdf & acadrust)".to_string()),
                ..Default::default()
            },
        }
    }
}
