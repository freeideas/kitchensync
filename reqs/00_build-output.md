# Build Requirements

Requires the contents of the `./released/` directory to contain the specified files and only the specified files.

## $REQ_BUILD_002: Linux Binary
**Source:** ./README.md (Section: "Building")

A build on Linux produces the file `./released/kitchensync.linux`.

## $REQ_BUILD_003: Windows Binary
**Source:** ./README.md (Section: "Building")

A build on Windows produces the file `./released/kitchensync.exe`.

## $REQ_BUILD_004: macOS Binary
**Source:** ./README.md (Section: "Building")

A build on macOS produces the file `./released/kitchensync.mac`.

## $REQ_BUILD_006: Single Platform Binary Per Build
**Source:** ./specs/build.md (Section: "Output")

A build produces exactly one binary in `./released/` — the one for the current platform.

## $REQ_BUILD_007: Help Flag
**Source:** ./specs/build.md (Section: "Help Flag")

Running the binary with `-h` or `--help` prints the help text defined in `specs/help.md` and exits with status 0.
