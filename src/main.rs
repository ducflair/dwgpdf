use clap::{Parser, ValueEnum};
use dwgpdf::{
    convert_file_to_pdf, ColorMode, ConversionMode, ConversionOptions, Metadata, Orientation,
    PageSize,
};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "dwgpdf", author, version, about = "High-performance CAD (DXF, DWG, DWF) to PDF converter in Rust using hipdf", long_about = None)]
struct Cli {
    /// Input CAD file (.dxf, .dwg, .dwf)
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output PDF file (defaults to input with .pdf extension)
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,

    /// Paper size
    #[arg(short, long, value_enum, default_value_t = CliPageSize::Auto)]
    page: CliPageSize,

    /// Page orientation
    #[arg(long, value_enum, default_value_t = CliOrientation::Auto)]
    orientation: CliOrientation,

    /// Color mode
    #[arg(long, value_enum, default_value_t = CliColorMode::Normal)]
    color: CliColorMode,

    /// Explicit scale (e.g. 0.02 for 1:50)
    #[arg(long)]
    scale: Option<f64>,

    /// Background color in hex (e.g. #ffffff)
    #[arg(long, default_value = "#ffffff")]
    background: String,

    /// Document title metadata
    #[arg(long)]
    title: Option<String>,

    /// Document author metadata
    #[arg(long)]
    author: Option<String>,

    /// Enable CAD entity and layer lineweights (by default lines are rendered as uniform thin hairlines like Three.js/CAD canvas)
    #[arg(long, default_value_t = false)]
    use_lineweights: bool,

    /// Line width scale factor
    #[arg(long, default_value_t = 1.0)]
    line_width_scale: f64,

    /// Minimum / uniform line width in points (default: 0.0 for laser-thin hairline like CAD viewer)
    #[arg(long, default_value_t = 0.0)]
    min_line_width: f64,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum CliPageSize {
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
}

impl From<CliPageSize> for PageSize {
    fn from(s: CliPageSize) -> Self {
        match s {
            CliPageSize::Auto => PageSize::Auto,
            CliPageSize::A0 => PageSize::A0,
            CliPageSize::A1 => PageSize::A1,
            CliPageSize::A2 => PageSize::A2,
            CliPageSize::A3 => PageSize::A3,
            CliPageSize::A4 => PageSize::A4,
            CliPageSize::A5 => PageSize::A5,
            CliPageSize::Letter => PageSize::Letter,
            CliPageSize::Legal => PageSize::Legal,
            CliPageSize::Tabloid => PageSize::Tabloid,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum CliOrientation {
    Auto,
    Portrait,
    Landscape,
}

impl From<CliOrientation> for Orientation {
    fn from(o: CliOrientation) -> Self {
        match o {
            CliOrientation::Auto => Orientation::Auto,
            CliOrientation::Portrait => Orientation::Portrait,
            CliOrientation::Landscape => Orientation::Landscape,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum CliColorMode {
    Normal,
    Monochrome,
    Grayscale,
    Invert,
}

impl From<CliColorMode> for ColorMode {
    fn from(c: CliColorMode) -> Self {
        match c {
            CliColorMode::Normal => ColorMode::Normal,
            CliColorMode::Monochrome => ColorMode::Monochrome,
            CliColorMode::Grayscale => ColorMode::Grayscale,
            CliColorMode::Invert => ColorMode::Invert,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let output_path = cli.output.unwrap_or_else(|| {
        let mut p = cli.input.clone();
        p.set_extension("pdf");
        p
    });

    let options = ConversionOptions {
        page_size: cli.page.into(),
        orientation: cli.orientation.into(),
        color_mode: cli.color.into(),
        scale: cli.scale,
        background_color: Some(cli.background),
        mode: ConversionMode::Plot,
        metadata: Metadata {
            title: cli.title,
            author: cli.author,
            creator: Some("dwgpdf".to_string()),
            ..Default::default()
        },
        use_lineweights: cli.use_lineweights,
        line_width_scale: cli.line_width_scale,
        min_line_width_pt: cli.min_line_width,
        ..Default::default()
    };

    println!("Converting {} -> {}...", cli.input.display(), output_path.display());
    let pdf_bytes = convert_file_to_pdf(&cli.input, &options)?;
    std::fs::write(&output_path, &pdf_bytes)?;
    println!("Successfully generated PDF ({} bytes)", pdf_bytes.len());

    Ok(())
}
