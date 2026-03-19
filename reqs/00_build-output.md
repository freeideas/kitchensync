# Build Requirements

Requires the contents of the `./released/` directory to contain the specified files and only the specified files after each build.

## $REQ_BUILD_001: Platform Binary Produced
**Source:** ./specs/build.md (Section: "Output")

A build produces the platform-appropriate binary in `./released/`: `kitchensync.linux` on Linux, `kitchensync.exe` on Windows, `kitchensync.mac` on macOS.

## $REQ_BUILD_002: Released Directory Contains Only Platform Binary
**Source:** ./specs/build.md (Section: "Process")

After a build, `./released/` contains only the platform-appropriate binary and no other files.

## $REQ_BUILD_005: Help Flag Embedded at Build Time
**Source:** ./specs/build.md (Section: "Help Flag")

The help text defined in `specs/help.md` is embedded in the binary at build time. `-h` or `--help` prints it and exits 0.
