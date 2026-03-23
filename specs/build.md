# Build

## Output

A build produces one of the following binaries in `./released/`:

| Platform | Output              |
| -------- | ------------------- |
| Linux    | `kitchensync.linux` |
| Windows  | `kitchensync.exe`   |
| macOS    | `kitchensync.mac`   |

## Process

1. Delete only the current platform's binary from `./released/` (preserve other platforms' binaries)
2. Build the binary for the current platform
3. Copy to `./released/` with the platform-appropriate name

## Help Flag

`-h`, `--help`, or no arguments at all prints the help text defined in `specs/help.md` (embedded at build time) and exits 0.
