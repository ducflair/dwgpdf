# Local Test Fixtures

Place any of your own CAD files (`.dxf`, `.dwg`, `.dwf`) in this directory.

- **Untouched**: The automated fixture fetcher will never modify, overwrite, or delete any files in this directory.
- **Uncommitted**: All files here are ignored by Git (per `.gitignore`), so your local files remain strictly on your machine.
- **Automatically Tested**: The integration test suite (`cargo test`) dynamically discovers and runs conversion tests on all files placed in this folder alongside the official fixtures.
