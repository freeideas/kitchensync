# Build Output Requirements

Requirements for the contents of the `./released/` directory after a build.

## $REQ_BUILD_002: Current-Platform Binary
**Source:** ./specs/build.md (Section: "Output")

A default build (no flags) produces a single binary in `./released/` for the current platform:
- Linux: `kitchensync.linux`
- Windows: `kitchensync.exe`
- macOS: `kitchensync.mac`

Only the current platform's binary is guaranteed to exist after a default build. Binaries for other platforms may or may not be present.

## $REQ_BUILD_005: Single Binary No Dependencies
**Source:** ./README.md (Section: "Why KitchenSync?")

Each platform binary is a single file with no external dependencies. No Cygwin, WSL, or MinGW required on Windows.

## $REQ_BUILD_007: Cross-Compile All Platforms
**Source:** ./specs/build.md (Section: "Process")

When `--all` is passed to the build, all three platform binaries are produced.
