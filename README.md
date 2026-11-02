# dwgpdf

High-performance, pure Rust converter from CAD formats (**DXF**, **DWG**, **DWF**) to vector **PDF**, powered by [`acadrust`](https://crates.io/crates/acadrust) and [`hipdf`](https://crates.io/crates/hipdf).

Inspired by the rendering architecture of Ducflair's TypeScript DXF renderer (`dxf-renderer`) and the PDF testing pipeline of `ducpdf`.

---

## Features

- **Direct Native Parsing**: Uses `acadrust` directly to parse both **DXF** (ASCII & binary) and **DWG** (AutoCAD R13 through 2018+) into native CAD structures without requiring external conversion binaries or intermediate formats.
- **DWF Package Support**: Parses Autodesk Design Web Format (`.dwf`) 2D/3D archives, extracting sheets, manifest metadata, and high-fidelity raster/vector renderings.
- **Vector PDF Output with `hipdf`**:
  - Exact Bézier curve approximations for circles, arcs, elliptical arcs, and polyline bulge segments.
  - Full AutoCAD Color Index (ACI 1-255) and TrueColor RGB color space resolution.
  - Optional PDF Layers (Optional Content Groups - OCG) mapping each CAD layer to an interactive, toggleable layer in PDF viewers (Acrobat, macOS Preview, PDF.js).
  - Clean MTEXT and TEXT formatting with CAD escape sequence decoding (`%%c`, `%%d`, `%%p`, `\P`, `\C`, `\U+XXXX`).
  - CAD linetype mapping to PDF dash patterns (`DASHED`, `DOTTED`, `DASHDOT`, `DIVIDE`, `CENTER`, `BORDER`, etc.).
  - Nested block reference (`INSERT`) and dimension (`DIMENSION`) hierarchy resolution with full 2D affine transformation matrices (translation, rotation, non-uniform scaling).
- **Flexible Plot & Page Options**:
  - Auto bounding-box fitting or ISO standard page sizes (A0, A1, A2, A3, A4, A5, Letter, Legal, Tabloid, Custom).
  - Orientations: Auto, Portrait, Landscape.
  - Color modes: Normal, Monochrome, Grayscale, Invert.
  - Plot mode (fit drawing bounds) vs Crop mode (user-defined coordinate window).
  - Document metadata embedding (Title, Author, Subject, Creator).

---

## Installation & CLI Usage

### Build from source
```bash
cargo build --release
```

### CLI Command
```bash
# Convert DXF to PDF (auto-sizing page and margins)
dwgpdf drawing.dxf -o drawing.pdf

# Convert DWG to PDF on A3 paper in Landscape orientation
dwgpdf floorplan.dwg -o floorplan.pdf -p a3 --orientation landscape

# Convert DWF package to PDF
dwgpdf system.dwf -o system.pdf

# Convert with monochrome color mode and custom background color
dwgpdf drawing.dxf -o drawing.pdf --color monochrome --background "#ffffff"
```

---

## Library Usage

Add `dwgpdf` to your `Cargo.toml`:

```toml
[dependencies]
dwgpdf = { path = "../dwgpdf" }
```

### Example: Converting CAD File to PDF

```rust
use dwgpdf::{convert_file_to_pdf, ConversionOptions, PageSize, Orientation};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = ConversionOptions::default();
    options.page_size = PageSize::A4;
    options.orientation = Orientation::Landscape;
    options.export_layers = true; // Enables interactive PDF layers

    let pdf_bytes = convert_file_to_pdf(Path::new("drawing.dxf"), &options)?;
    std::fs::write("output.pdf", pdf_bytes)?;

    println!("PDF created successfully!");
    Ok(())
}
```

### Example: Converting Raw Bytes in Memory

```rust
use dwgpdf::{convert_to_pdf, ConversionOptions, ColorMode};

fn convert_in_memory(cad_bytes: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut options = ConversionOptions::default();
    options.color_mode = ColorMode::Normal;

    // Automatically detects DXF, DWG, or DWF from byte magic headers
    let pdf_bytes = convert_to_pdf(cad_bytes, &options)?;
    Ok(pdf_bytes)
}
```

---

## Testing & Fixtures

The test suite runs comprehensive conversions and structural validations against real CAD files from [ducflair/fixtures](https://github.com/ducflair/fixtures):

- **DXF (11 fixtures)**: `columns_R2007.dxf`, `hatches_2.dxf`, `house-plan.dxf` (17MB), `mtext_complex_inline_codes.dxf`, `text.dxf`, `text_fonts.dxf`, `text_mirror_shx_font.dxf`, `text_mirror_true_type_font.dxf`, `text_negative_extrusion.dxf`, `uncommon.dxf`, `wipeout_door.dxf`.
- **DWG (9 fixtures)**: `Electrical-Plan-Legend.dwg`, `People-and-Wheelchairs-Dimensions.dwg`, `sample_AC1014.dwg`, `sample_AC1015.dwg`, `sample_AC1018.dwg`, `sample_AC1021.dwg`, `sample_AC1024.dwg`, `sample_AC1027.dwg`, `sample_AC1032.dwg`.
- **DWF (1 fixture)**: `FlyAshSystem.dwf`.

### Running Tests

```bash
# Run integration tests across all fixtures
cargo test

# Fast release mode (recommended for large drawings like house-plan.dxf)
cargo test --release
```

### Automated Fixture Management

- **Automated Pulling**: If any official fixture is missing locally in `tests/fixtures/`, the test suite automatically pulls it from GitHub or a local mirror before running.
- **Adding Your Own Local Fixtures**:
  - Simply place your CAD files in `tests/fixtures/local/` (or name them with `.local.`, e.g. `my_drawing.local.dwg`).
  - **Untouched**: The automated fetcher will never modify, overwrite, or delete your files.
  - **Uncommitted**: `tests/fixtures/local/` is ignored by `.gitignore`, ensuring your private files are never committed to Git.
  - **Auto-Tested**: The test runner dynamically discovers and converts any files found in `tests/fixtures/local/` alongside the official fixtures.
