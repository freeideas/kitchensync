# Build Requirements

Requires the contents of the `./released/` directory to contain the specified files and only the specified files after a build.

## $REQ_BUILD_003: Linux Binary
**Source:** ./specs/build.md (Section: "Output")

On Linux, the build produces `./released/kitchensync.linux`.

## $REQ_BUILD_004: Windows Binary
**Source:** ./specs/build.md (Section: "Output")

On Windows, the build produces `./released/kitchensync.exe`.

## $REQ_BUILD_005: macOS Binary
**Source:** ./specs/build.md (Section: "Output")

On macOS, the build produces `./released/kitchensync.mac`.

## $REQ_BUILD_006: Single Binary Per Platform
**Source:** ./specs/build.md (Section: "Output")

A build produces exactly one binary in `./released/` — the one for the current platform. No other files should be present in `./released/` after a build.