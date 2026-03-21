# Build Requirements

Requires the contents of the `./released/` directory to contain the specified files and only the specified files after a build.

## $REQ_BUILD_001: Linux Binary
**Source:** ./specs/build.md (Section: "Output")

A build on Linux produces the file `./released/kitchensync.linux`.

## $REQ_BUILD_002: Windows Binary
**Source:** ./specs/build.md (Section: "Output")

A build on Windows produces the file `./released/kitchensync.exe`.

## $REQ_BUILD_003: macOS Binary
**Source:** ./specs/build.md (Section: "Output")

A build on macOS produces the file `./released/kitchensync.mac`.

## $REQ_BUILD_004: Single Platform Binary
**Source:** ./specs/build.md (Section: "Process")

Each build produces exactly one binary for the current platform. The `./released/` directory contains only the platform-appropriate binary after a build.
