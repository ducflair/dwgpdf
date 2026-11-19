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

Tests use checked-in official DXF, DWG, and DWF fixtures. They do not download external data or scan or convert files in `tests/fixtures/local/`. One isolated test checks local-folder discovery and conversion using a temporary directory and a copy of a checked-in fixture.

## Releases

Releases are generated from Conventional Commits on `main`: `fix:` creates a patch release, `feat:` a minor release, and a `BREAKING CHANGE:` footer creates a major release. The release workflow publishes the matching version to npm and crates.io and attaches both package archives to the GitHub Release. Configure the repository secrets `NPM_TOKEN` and `CARGO_REGISTRY_TOKEN`; GitHub Releases use the workflow's `GITHUB_TOKEN`.

## License

MIT
