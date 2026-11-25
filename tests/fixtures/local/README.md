# Local fixtures

This directory is reserved for CAD files you keep on your machine. Its contents are ignored by Git and are optional during tests: `test_all_local_fixtures` converts any `.dxf`, `.dwg`, or `.dwf` files present here to `tests/output/local.<ext>.<filename>.pdf`, and passes cleanly when empty.
