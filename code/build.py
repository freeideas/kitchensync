#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "requests",
# ]
# ///

import sys
import os
import platform
import subprocess
import shutil
import zipfile
import tarfile
from pathlib import Path

# Fix Windows console encoding for Unicode characters
if sys.stdout.encoding != 'utf-8':
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')

import requests

# Determine paths
SCRIPT_DIR = Path(__file__).parent.resolve()
PROJECT_ROOT = SCRIPT_DIR.parent
TOOLS_DIR = PROJECT_ROOT / "tools"
COMPILER_DIR = TOOLS_DIR / "compiler"
MINGW_DIR = TOOLS_DIR / "mingw64"
RELEASED_DIR = PROJECT_ROOT / "released"
CODE_DIR = SCRIPT_DIR

# Platform detection
SYSTEM = platform.system().lower()
IS_WINDOWS = SYSTEM == "windows"
IS_LINUX = SYSTEM == "linux"
IS_MACOS = SYSTEM == "darwin"


def print_step(msg: str):
    print(f"✓ {msg}")


def print_error(msg: str):
    print(f"✗ {msg}", file=sys.stderr)


def download_file(url: str, dest: Path) -> bool:
    """Download a file from URL to dest."""
    print(f"  Downloading {url}...")
    try:
        resp = requests.get(url, stream=True, timeout=300)
        resp.raise_for_status()
        with open(dest, "wb") as f:
            for chunk in resp.iter_content(chunk_size=8192):
                f.write(chunk)
        return True
    except Exception as e:
        print_error(f"Download failed: {e}")
        return False


def extract_zip(archive: Path, dest: Path):
    """Extract a zip archive."""
    with zipfile.ZipFile(archive, "r") as zf:
        zf.extractall(dest)


def extract_tar(archive: Path, dest: Path):
    """Extract a tar archive (gzip or xz)."""
    with tarfile.open(archive, "r:*") as tf:
        tf.extractall(dest)


def setup_rust_windows() -> Path:
    """Set up Rust toolchain on Windows using rustup-init.exe."""
    rustup_home = COMPILER_DIR / "rustup"
    cargo_home = COMPILER_DIR / "cargo"
    cargo_bin = cargo_home / "bin" / "cargo.exe"

    if cargo_bin.exists():
        print_step("Rust toolchain already installed")
        return cargo_bin

    print("Setting up Rust toolchain for Windows...")

    # Create directories
    rustup_home.mkdir(parents=True, exist_ok=True)
    cargo_home.mkdir(parents=True, exist_ok=True)
    MINGW_DIR.mkdir(parents=True, exist_ok=True)

    # Download rustup-init.exe
    rustup_init = COMPILER_DIR / "rustup-init.exe"
    if not rustup_init.exists():
        url = "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-gnu/rustup-init.exe"
        if not download_file(url, rustup_init):
            sys.exit(1)

    # Download MinGW-w64
    mingw_gcc = MINGW_DIR / "bin" / "gcc.exe"
    if not mingw_gcc.exists():
        print("  Downloading MinGW-w64...")
        mingw_url = "https://github.com/niXman/mingw-builds-binaries/releases/download/13.2.0-rt_v11-rev1/x86_64-13.2.0-release-posix-seh-ucrt-rt_v11-rev1.7z"
        mingw_archive = TOOLS_DIR / "mingw.7z"

        if not download_file(mingw_url, mingw_archive):
            # Try alternative: download a zip version
            mingw_url_alt = "https://github.com/brechtsanders/winlibs_mingw/releases/download/13.2.0posix-18.1.1-11.0.1-ucrt-r4/winlibs-x86_64-posix-seh-gcc-13.2.0-mingw-w64ucrt-11.0.1-r4.zip"
            mingw_archive = TOOLS_DIR / "mingw.zip"
            if not download_file(mingw_url_alt, mingw_archive):
                print_error("Failed to download MinGW-w64")
                sys.exit(1)

        # Extract
        print("  Extracting MinGW-w64...")
        if mingw_archive.suffix == ".zip":
            extract_zip(mingw_archive, TOOLS_DIR)
            # Move contents to mingw64
            for item in (TOOLS_DIR / "mingw64").iterdir():
                pass  # Already in place
        else:
            # Need 7z to extract
            subprocess.run(["7z", "x", str(mingw_archive), f"-o{TOOLS_DIR}"], check=True)

        mingw_archive.unlink()

    # Install Rust
    print("  Installing Rust toolchain...")
    env = os.environ.copy()
    env["RUSTUP_HOME"] = str(rustup_home)
    env["CARGO_HOME"] = str(cargo_home)

    result = subprocess.run(
        [
            str(rustup_init),
            "-y",
            "--default-toolchain", "stable-x86_64-pc-windows-gnu",
            "--no-modify-path",
        ],
        env=env,
        capture_output=True,
        text=True,
        encoding='utf-8',
    )

    if result.returncode != 0:
        print_error(f"rustup-init failed: {result.stderr}")
        sys.exit(1)

    # Create cargo config for linker
    cargo_config_dir = CODE_DIR / ".cargo"
    cargo_config_dir.mkdir(exist_ok=True)
    cargo_config = cargo_config_dir / "config.toml"
    cargo_config.write_text(f'''[target.x86_64-pc-windows-gnu]
linker = "{(MINGW_DIR / "bin" / "gcc.exe").as_posix()}"
''')

    print_step("Rust toolchain installed")
    return cargo_bin


def setup_rust_linux() -> Path:
    """Set up Rust toolchain on Linux."""
    rustup_home = COMPILER_DIR / "rustup"
    cargo_home = COMPILER_DIR / "cargo"
    cargo_bin = cargo_home / "bin" / "cargo"

    if cargo_bin.exists():
        print_step("Rust toolchain already installed")
        return cargo_bin

    print("Setting up Rust toolchain for Linux...")

    rustup_home.mkdir(parents=True, exist_ok=True)
    cargo_home.mkdir(parents=True, exist_ok=True)

    # Download rustup-init
    rustup_init = COMPILER_DIR / "rustup-init"
    if not rustup_init.exists():
        url = "https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init"
        if not download_file(url, rustup_init):
            sys.exit(1)
        rustup_init.chmod(0o755)

    # Install
    env = os.environ.copy()
    env["RUSTUP_HOME"] = str(rustup_home)
    env["CARGO_HOME"] = str(cargo_home)

    result = subprocess.run(
        [str(rustup_init), "-y", "--no-modify-path"],
        env=env,
        capture_output=True,
        text=True,
        encoding='utf-8',
    )

    if result.returncode != 0:
        print_error(f"rustup-init failed: {result.stderr}")
        sys.exit(1)

    print_step("Rust toolchain installed")
    return cargo_bin


def setup_rust_macos() -> Path:
    """Set up Rust toolchain on macOS."""
    rustup_home = COMPILER_DIR / "rustup"
    cargo_home = COMPILER_DIR / "cargo"
    cargo_bin = cargo_home / "bin" / "cargo"

    if cargo_bin.exists():
        print_step("Rust toolchain already installed")
        return cargo_bin

    print("Setting up Rust toolchain for macOS...")

    rustup_home.mkdir(parents=True, exist_ok=True)
    cargo_home.mkdir(parents=True, exist_ok=True)

    # Download rustup-init
    rustup_init = COMPILER_DIR / "rustup-init"
    if not rustup_init.exists():
        url = "https://static.rust-lang.org/rustup/dist/x86_64-apple-darwin/rustup-init"
        if not download_file(url, rustup_init):
            sys.exit(1)
        rustup_init.chmod(0o755)

    # Install
    env = os.environ.copy()
    env["RUSTUP_HOME"] = str(rustup_home)
    env["CARGO_HOME"] = str(cargo_home)

    result = subprocess.run(
        [str(rustup_init), "-y", "--no-modify-path"],
        env=env,
        capture_output=True,
        text=True,
        encoding='utf-8',
    )

    if result.returncode != 0:
        print_error(f"rustup-init failed: {result.stderr}")
        sys.exit(1)

    print_step("Rust toolchain installed")
    return cargo_bin


def setup_rust() -> Path:
    """Set up Rust toolchain for the current platform."""
    TOOLS_DIR.mkdir(parents=True, exist_ok=True)
    COMPILER_DIR.mkdir(parents=True, exist_ok=True)

    if IS_WINDOWS:
        return setup_rust_windows()
    elif IS_LINUX:
        return setup_rust_linux()
    elif IS_MACOS:
        return setup_rust_macos()
    else:
        print_error(f"Unsupported platform: {SYSTEM}")
        sys.exit(1)


def build(cargo_bin: Path) -> Path:
    """Build the project and return the binary path."""
    print("Building KitchenSync...")

    # Set up environment
    env = os.environ.copy()
    env["RUSTUP_HOME"] = str(COMPILER_DIR / "rustup")
    env["CARGO_HOME"] = str(COMPILER_DIR / "cargo")

    # Add MinGW to PATH on Windows
    if IS_WINDOWS:
        mingw_bin = MINGW_DIR / "bin"
        if mingw_bin.exists():
            env["PATH"] = f"{mingw_bin};{env.get('PATH', '')}"

    # Run cargo build
    result = subprocess.run(
        [str(cargo_bin), "build", "--release"],
        cwd=CODE_DIR,
        env=env,
        capture_output=True,
        text=True,
        encoding='utf-8',
    )

    if result.returncode != 0:
        print_error("Build failed:")
        print(result.stdout)
        print(result.stderr)
        sys.exit(1)

    # Find the binary
    target_dir = CODE_DIR / "target" / "release"
    if IS_WINDOWS:
        binary = target_dir / "kitchensync.exe"
    else:
        binary = target_dir / "kitchensync"

    if not binary.exists():
        print_error(f"Binary not found at {binary}")
        sys.exit(1)

    print_step("Build successful")
    return binary


def copy_to_released(binary: Path):
    """Copy the binary to released/ with platform-appropriate name."""
    # Clear released directory
    if RELEASED_DIR.exists():
        shutil.rmtree(RELEASED_DIR)
    RELEASED_DIR.mkdir(parents=True)

    # Determine output name
    if IS_WINDOWS:
        output_name = "kitchensync.exe"
    elif IS_LINUX:
        output_name = "kitchensync.linux"
    elif IS_MACOS:
        output_name = "kitchensync.mac"
    else:
        output_name = "kitchensync"

    output_path = RELEASED_DIR / output_name
    shutil.copy2(binary, output_path)

    # Make executable on Unix
    if not IS_WINDOWS:
        output_path.chmod(0o755)

    print_step(f"Binary copied to {output_path}")


def verify_binary(binary_path: Path):
    """Verify the binary works by running --help."""
    print("Verifying binary...")

    result = subprocess.run(
        [str(binary_path), "--help"],
        capture_output=True,
        text=True,
        encoding='utf-8',
    )

    if result.returncode != 0:
        print_error(f"Binary verification failed: {result.stderr}")
        sys.exit(1)

    if "kitchensync" not in result.stdout.lower():
        print_error("Binary output doesn't look right")
        sys.exit(1)

    print_step("Binary verified")


def main():
    print("=" * 60)
    print("KitchenSync Build Script")
    print("=" * 60)

    # Step 1: Set up Rust
    cargo_bin = setup_rust()

    # Step 2: Build
    binary = build(cargo_bin)

    # Step 3: Copy to released
    copy_to_released(binary)

    # Step 4: Verify
    if IS_WINDOWS:
        released_binary = RELEASED_DIR / "kitchensync.exe"
    elif IS_LINUX:
        released_binary = RELEASED_DIR / "kitchensync.linux"
    elif IS_MACOS:
        released_binary = RELEASED_DIR / "kitchensync.mac"
    else:
        released_binary = RELEASED_DIR / "kitchensync"

    verify_binary(released_binary)

    print("=" * 60)
    print("Build complete!")
    print(f"Output: {released_binary}")
    print("=" * 60)


if __name__ == "__main__":
    main()
