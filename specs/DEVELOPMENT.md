# Developing KitchenSync

General advice for anyone (person or agent) working on this project. The other files in `specs/` describe what the program must do; this one describes how we work on it.

## The specs

- `sync.md` defines the command line, startup, run phases, transports, logging, dry-run behavior, and error handling.
- `multi-tree-sync.md` defines traversal, decisions, excludes, subordinate peers, BAK cleanup, and manifest updates during sync.
- `manifest.md` defines the per-directory manifest: its format, which directories have one, how it is read and replaced, tombstones, rollback, and the timestamp format used everywhere.
- `concurrency.md` defines copy concurrency, fallback connection behavior, listing concurrency, progress output, retries, and trace logging.
- `help.md` defines the exact help screen.
- `TESTING-GUIDELINES.md` defines constraints for SFTP tests.
- `SCENARIOS.md` lists the end-to-end test scenarios.

## Repository layout

- `README.md` is the entry point: what KitchenSync is, how to run it, and the index of specs.
- `specs/` is the source of truth for behavior. If code and a spec disagree, the spec wins until the spec is changed.
- `extart/` holds supplied helper artifacts used by tests, such as the ephemeral SFTP server script.
- `released/` holds the shipped binaries, one per platform (see below).
- `code/` is the Rust program itself: `code/Cargo.toml`, `code/src/`, and Cargo's `code/target/` build output. Keeping it in one folder stops the compiler from scattering files across the repository root.
- `tests/` holds the end-to-end scenario tests, run with `uv run tests/run.py`.
- `tools/` holds a local, portable toolchain (see below). It is never committed.

## Toolchain lives under `./tools/`

Development uses a portable Rust toolchain and any other build or test tools placed under `./tools/` in the repository root, rather than whatever happens to be installed system-wide. The reasons: every machine builds with the same compiler version, nothing needs administrator rights or a global install, and a fresh clone plus a copy of `tools/` is a complete working environment.

Rules that follow from this:

- Build and test commands must find `cargo`, `rustc`, and friends under `./tools/`, for example by putting the toolchain's `bin` directory first on `PATH` for the duration of the command. Do not rely on `rustup`, Homebrew, or a system package for the compiler.
- If a needed tool is missing from `./tools/`, add it there as a portable (unpacked, self-contained) install. Do not install it globally and do not check it in.
- `./tools/` is listed in `.gitignore` and stays out of GitHub. Its contents are large and platform-specific, so each machine keeps its own copy.
- Python helpers are run with `uv`, and single-file scripts declare their dependencies inline (see `extart/ephemeral-sftp-server.py` for the pattern). If `uv` is not on the machine, it also belongs under `./tools/`.

## Released binaries live under `./released/` and are committed

The build writes one executable per platform into `./released/`, named so all three can sit side by side:

| Platform | File                                                                                                  |
| -------- | ----------------------------------------------------------------------------------------------------- |
| Windows  | `released/kitchensync.exe`                                                                            |
| macOS    | `released/kitchensync.mac`                                                                            |
| Linux    | `released/kitchensync.linux` (a normal ELF executable, the format every mainstream distribution runs) |

The macOS file is a plain single-file executable that happens to end in `.app`; it is not an application bundle folder, so run it from a shell rather than double-clicking it in Finder. There is no cross-compiling: each file is built on its own platform with that machine's `./tools/` toolchain, and a machine only rebuilds the file for its own platform. Whichever files exist are committed so that a clone from GitHub includes ready-to-run binaries without needing the toolchain.

Keep them small: build in release mode with symbols stripped (the `[profile.release]` section in `Cargo.toml` does this) and avoid dependencies that bloat the executable. GitHub warns above 50 MB and refuses above 100 MB; the target is a few MB each.

Rebuild and recommit a binary whenever the code that produces it changes, so a committed file never lags behind the source.

Build with the toolchain on `PATH`:

```
export PATH="$PWD/tools/rust/bin:$PATH"
export CARGO_HOME="$PWD/tools/cargo-home"
cargo build --release --manifest-path code/Cargo.toml
```

The `CARGO_HOME` line keeps downloaded crates under `./tools/` too, instead of the user's home directory. `code/build.py` runs these steps and copies the result to `./released/` under the right name.

On Windows the SSH library's crypto backend (`aws-lc-sys`, pulled in by `russh`) compiles assembly and needs NASM, plus the MSVC linker from Visual Studio Build Tools. Unzip a NASM release (https://www.nasm.us/pub/nasm/releasebuilds/) so that `./tools/nasm/nasm.exe` exists; `code/build.py` puts that directory on `PATH` for the build. The Rust toolchain itself is the standalone installer from https://static.rust-lang.org/dist/ (the `x86_64-pc-windows-msvc` tarball), installed with `install.sh --prefix=./tools/rust`. On Windows that script (run through Git's `bash.exe`) crawls and can stall after the docs component; the reliable way is to unpack the tarball with the built-in `tar.exe` and `robocopy` the `rustc`, `rust-std-*`, `cargo`, `rustfmt-preview` and `clippy-preview` component folders into `./tools/rust` (each component's contents merge into the same `bin/`, `lib/` tree; skip `manifest.in`).

## Tests

Tests run the released binary for the current platform directly from `./released/` and observe its exit code, stdout, stderr, and the filesystem under directories they create. They do not link against the code. All diagnostics and progress output go to stdout and stderr must remain empty. SFTP tests must follow `specs/TESTING-GUIDELINES.md` and use the ephemeral server in `extart/` rather than any real host.

## Writing style for documents

Write Markdown without hard-wrapping: one paragraph per line, one list item per line. Use plain language and explain any technical term the first time it appears.
