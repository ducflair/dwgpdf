# dwgpdf

`dwgpdf` converts DXF, DWG, and DWF drawings to PDF. It is available as a Rust library, a command-line tool, and a WebAssembly package for JavaScript bundlers.

## Install

### Rust library

```toml
[dependencies]
dwgpdf = "0.1"
```

### Command-line tool

```sh
cargo install dwgpdf
dwgpdf drawing.dwg -o drawing.pdf --page a3 --orientation landscape
```

## Rust usage

Convert a file on disk:

```rust
use dwgpdf::{convert_file_to_pdf, ConversionOptions};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pdf = convert_file_to_pdf(Path::new("drawing.dxf"), &ConversionOptions::default())?;
    std::fs::write("drawing.pdf", pdf)?;
    Ok(())
}
```

Or convert bytes directly, which is also the API used by the WebAssembly bindings:

```rust
use dwgpdf::{convert_to_pdf, ConversionOptions};

let pdf = convert_to_pdf(&cad_bytes, &ConversionOptions::default())?;
```

## JavaScript and WebAssembly

The npm package targets JavaScript bundlers and accepts CAD data as a `Uint8Array`:

```sh
npm install dwgpdf
```

```js
import { convert, convertWithOptions } from "dwgpdf";

const input = new Uint8Array(await file.arrayBuffer());
const pdf = convert(input); // Uint8Array

// Options may be supplied as a partial ConversionOptions object.
const monochromePdf = convertWithOptions(input, { color_mode: "Monochrome" });
```

To build the npm package from source, install [`wasm-pack`](https://rustwasm.github.io/wasm-pack/) and run:

```sh
npm pack
```

The package build targets `wasm32-unknown-unknown` and JavaScript environments with Web Crypto support, including modern browsers and Node.js 19 or newer.

## Conversion options

The Rust API exposes page size and orientation, plot/crop mode, color mode, lineweights, PDF layers, and document metadata through `ConversionOptions`. The CLI exposes the commonly used page, orientation, color, scale, background, and lineweight options; run `dwgpdf --help` for the full list.

## Tests

Run the test suite with:

```sh
cargo test
```

Tests use checked-in official DXF, DWG, and DWF fixtures, plus any optional files placed in `tests/fixtures/local/` (never required or hardcoded, so CI passes when `local/` is empty).

To convert all checked-in fixtures (and any optional `tests/fixtures/local/` files) into `tests/output/`:

```sh
cargo test --release --test test_integration test_all_
```

Generated PDFs are written flat in `tests/output/` using format prefixes:
- `tests/output/dwg.<filename>.pdf`
- `tests/output/dwf.<filename>.pdf`
- `tests/output/dxf.<filename>.pdf`
- `tests/output/local.<dwg|dwf|dxf>.<filename>.pdf` (when local fixtures are present)


## License

MIT
