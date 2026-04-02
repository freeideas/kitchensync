# Build

## Output

A build produces one of the following binaries in `./released/`:

| Platform | Output              |
| -------- | ------------------- |
| Linux    | `kitchensync.linux` |
| Windows  | `kitchensync.exe`   |
| macOS    | `kitchensync.mac`   |

## Process

By default, only the current platform's binary is built. Pass `--all` to cross-compile all three platforms.

1. Delete only the current platform's binary from `./released/` (preserve binaries from other platforms so that builds from multiple platforms accumulate)
2. Build the binary for the current platform: `go build -o ./released/<platform-binary> ./cmd/kitchensync`
3. If `--all` is specified, also cross-compile the other two platforms: `GOOS=<os> GOARCH=<arch> go build -o ./released/<binary> ./cmd/kitchensync` (skip any that already exist)

## Go Module

The module is `kitchensync`. Source lives under `./code/` (where `go.mod` lives). All `go build` commands run from `./code/`; output paths in step 2/3 are relative to the project root.

## Dependencies

Key Go libraries:
- `github.com/pkg/sftp` + `golang.org/x/crypto/ssh` — SFTP/SSH
- `github.com/mattn/go-sqlite3` or `modernc.org/sqlite` (pure Go, no CGO) — SQLite
- `github.com/cespare/xxhash/v2` — xxHash64 for path hashing
- `github.com/sabhiram/go-gitignore` — .syncignore pattern matching

### SSH known_hosts and HostKeyAlgorithms

**CRITICAL:** Go's `x/crypto/ssh/knownhosts` callback verifies the host key presented during the SSH handshake against `~/.ssh/known_hosts`. However, if the server offers multiple key types (e.g. ed25519, ecdsa, rsa) and Go's SSH client negotiates a different key type than what's recorded in `known_hosts`, the callback returns a "key mismatch" error -- even though the correct key IS in the file, just for a different algorithm.

**Fix:** Before connecting, read `~/.ssh/known_hosts` to find which key algorithms are recorded for the target host, and set `ssh.ClientConfig.HostKeyAlgorithms` to that list. This constrains the handshake to negotiate only key types that `known_hosts` can verify.

This is a well-known Go SSH pitfall. The `knownhosts.HostKeyAlgorithms()` function was added in later versions of `x/crypto` to solve this, but if your version lacks it, parse the file manually: match host entries, extract the key type field (column 2), and return the list.

## Help Flag

`-h`, `--help`, `/?`, or no arguments at all prints the help text defined in `specs/help.md` (embedded at build time) and exits 0.
