# Build Requirements

Requires the contents of the `./released/` directory to contain the specified files after a build.

## $REQ_BUILD_002: Linux Binary
**Source:** ./README.md (Section: "Building")

A build for Linux produces `./released/kitchensync.linux`.

## $REQ_BUILD_003: Windows Binary
**Source:** ./README.md (Section: "Building")

A build for Windows produces `./released/kitchensync.exe`.

## $REQ_BUILD_004: macOS Binary
**Source:** ./README.md (Section: "Building")

A build for macOS produces `./released/kitchensync.mac`.

## $REQ_BUILD_005: Platform-Selective Build Output
**Source:** ./specs/build.md (Section: "Process")

After building for one platform, binaries for other platforms remain in `./released/`, so that builds from multiple platforms accumulate.

## $REQ_BUILD_008: No Dependencies
**Source:** ./README.md (Section: "Why KitchenSync?")

The released binary is a single binary with no external dependencies. No Cygwin, no WSL, no MinGW required.
