# Download Compiler

Ensure the required compiler is available for building the project.

This project uses **Rust**. Download a portable Rust toolchain into
`./tools/compiler/`.

## Process

1. **Check what's already available** -- if `./tools/compiler/cargo/bin/rustc.exe`
   (or `rustc` on Unix) already exists, verify it works and stop
2. **Download portable Rust** -- use `rustup` to install a standalone toolchain
   into `./tools/compiler/`
3. **Install cbindgen** -- needed for generating C headers from Rust code
4. **Verify** -- run `rustc --version` and `cargo --version` from the downloaded location

## Requirements

- Do **not** rely on PATH; always use the downloaded compiler from `./tools/compiler/`
- Only if the WRONG tool is in `./tools/compiler/`, delete that directory and replace it
- Download portable/standalone builds (not system-wide installers)
- The `./tools/` directory is gitignored

## Target Layout

**IMPORTANT: All binaries must have .exe extension, even on Linux/macOS.**
Those OSes ignore the extension, and .exe makes it clear these are executables.
After installation, rename any binaries that lack the .exe extension.

```
./tools/compiler/
└── cargo/
    └── bin/
        ├── rustc.exe
        ├── cargo.exe
        ├── cbindgen.exe
        └── ...
```

## Installation Steps (Rust)

### Option A: rustup (preferred)

```bash
# Set CARGO_HOME and RUSTUP_HOME to keep it portable
export CARGO_HOME="./tools/compiler/cargo"
export RUSTUP_HOME="./tools/compiler/rustup"

# Download and run rustup-init
# On Unix:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path

# On Windows (PowerShell):
# Download rustup-init.exe from https://win.rustup.rs/x86_64
# Run: ./rustup-init.exe -y --no-modify-path
```

### After installation:

```bash
# Rename binaries to .exe if they don't already have the extension
# (on Linux/macOS, rustup produces extensionless binaries)
cd ./tools/compiler/cargo/bin/
for f in rustc cargo rustfmt clippy-driver cargo-clippy cargo-fmt; do
    if [ -f "$f" ] && [ ! -f "$f.exe" ]; then
        mv "$f" "$f.exe"
    fi
done

# Verify
./tools/compiler/cargo/bin/rustc.exe --version
./tools/compiler/cargo/bin/cargo.exe --version

# Install cbindgen for C header generation
./tools/compiler/cargo/bin/cargo.exe install cbindgen

# Rename cbindgen too
cd ./tools/compiler/cargo/bin/
if [ -f "cbindgen" ] && [ ! -f "cbindgen.exe" ]; then
    mv cbindgen cbindgen.exe
fi
```

## Windows Notes

- Use the MSVC target (default) -- do NOT use the GNU target
- Requires Visual Studio Build Tools or Visual Studio with C++ workload
- See project docs for MSVC environment setup details

## Verification

After installation, these commands must succeed:

```bash
./tools/compiler/cargo/bin/rustc.exe --version
./tools/compiler/cargo/bin/cargo.exe --version
./tools/compiler/cargo/bin/cbindgen.exe --version
```
