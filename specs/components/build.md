# Build

## Language

Rust. The project is a Cargo workspace (or single crate) rooted at `./code/`.

## Output

A build produces one of the following binaries in `./released/`:

| Platform | Output              |
| -------- | ------------------- |
| Linux    | `kitchensync.linux` |
| Windows  | `kitchensync.exe`   |
| macOS    | `kitchensync.mac`   |

## Process

By default, only the current platform's binary is built. Pass `--all` to cross-compile all three platforms.

1. Delete `./released/` entirely and recreate it
2. Build the binary for the current platform: `cargo build --release` from `./code/`
3. Copy the resulting binary from `target/release/` to `../released/<platform-binary>`
4. If `--all` is specified, also cross-compile the other two platforms:

| Target triple              | Binary              |
| -------------------------- | ------------------- |
| `x86_64-unknown-linux-gnu` | `kitchensync.linux` |
| `x86_64-pc-windows-msvc`   | `kitchensync.exe`   |
| `aarch64-apple-darwin`     | `kitchensync.mac`   |

`cargo build --release --target <triple>` for each.

## Dependencies

Key capabilities needed (choose appropriate crates):
- SSH/SFTP client -- for remote peer connections
- SQLite -- for snapshot database (WAL mode support required)
- xxHash64 -- for path hashing
- Gitignore-compatible pattern matching -- for .syncignore
- Filesystem change notifications -- for watch mode
- UUID v4 generation -- for TMP staging paths
- Async runtime -- for concurrent transfers and directory listings

## Help Flag

`-h`, `--help`, `/?`, or no arguments at all prints the help text defined in `specs/components/cli.md` (embedded at build time) and exits 0.
