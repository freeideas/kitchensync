# Build

## Output

A build produces exactly one file in `./released/` — the binary for the current platform:

| Platform | Output                         |
| -------- | ------------------------------ |
| Linux    | `./released/kitchensync.linux` |
| Windows  | `./released/kitchensync.exe`   |
| macOS    | `./released/kitchensync.mac`   |

## Process

1. Delete everything in `./released/`
2. Build the binary for the current platform
3. Copy the binary to `./released/` with the platform-appropriate name

## Help Flag

When invoked with `-h` or `--help`, the binary prints the help text to stdout and exits with code 0. The help text is the contents of `specs/help.txt` embedded verbatim at build time.
