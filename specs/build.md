# Build

## Output

A build produces one of the following binaries in `./released/`:

| Platform | Output              |
| -------- | ------------------- |
| Linux    | `kitchensync.linux` |
| Windows  | `kitchensync.exe`   |
| macOS    | `kitchensync.mac`   |

## Process

1. Delete only the current platform's binary from `./released/` (preserve binaries from other platforms so that builds from multiple platforms accumulate)
2. Build the binary for the current platform: `go build -o ./released/<platform-binary> ./cmd/kitchensync`
3. Cross-compilation: `GOOS=<os> GOARCH=<arch> go build -o ./released/<binary> ./cmd/kitchensync`

## Go Module

The module is `kitchensync`. Source lives under `./code/` (where `go.mod` lives). All `go build` commands run from `./code/`; output paths in step 2/3 are relative to the project root.

## Dependencies

Key Go libraries:
- `github.com/pkg/sftp` + `golang.org/x/crypto/ssh` — SFTP/SSH
- `github.com/mattn/go-sqlite3` or `modernc.org/sqlite` (pure Go, no CGO) — SQLite
- `github.com/cespare/xxhash/v2` — xxHash64 for path hashing
- `github.com/sabhiram/go-gitignore` — .syncignore pattern matching

## Help Flag

`-h`, `--help`, `/?`, or no arguments at all prints the help text defined in `specs/help.md` (embedded at build time) and exits 0.
