# Build Output Requirements

Requirements for the contents of the `./released/` directory after a build.

## $REQ_BUILD_002: Linux Binary
**Source:** ./README.md (Section: "Building")

A build for Linux produces the file `./released/kitchensync.linux`.

## $REQ_BUILD_003: Windows Binary
**Source:** ./README.md (Section: "Building")

A build for Windows produces the file `./released/kitchensync.exe`.

## $REQ_BUILD_004: macOS Binary
**Source:** ./README.md (Section: "Building")

A build for macOS produces the file `./released/kitchensync.mac`.

## $REQ_BUILD_005: Single Binary No Dependencies
**Source:** ./README.md (Section: "Why KitchenSync?")

Each platform binary is a single file with no external dependencies. No Cygwin, WSL, or MinGW required on Windows.

## $REQ_BUILD_007: Cross-Compile All Platforms
**Source:** ./specs/build.md (Section: "Process")

When `--all` is passed to the build, all three platform binaries are produced.
