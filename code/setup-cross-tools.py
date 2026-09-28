#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Adds the cross-compiling tools to ./tools/ so a Mac can build the Linux and
Windows binaries as well as its own.

Run as: uv run code/setup-cross-tools.py

It needs the portable Rust toolchain already in ./tools/rust/ (see
specs/DEVELOPMENT.md) and adds, only where missing:

- the Rust standard library for each cross target, matching the installed
  rustc version exactly, merged into ./tools/rust/
- Zig, unpacked into ./tools/zig/, used as the C compiler and linker for the
  foreign targets (it ships the Linux and Windows C libraries itself)
- cargo-zigbuild, into ./tools/bin/, which makes cargo use Zig that way

Safe to run again: anything already present is left alone. After a Rust
toolchain upgrade, rerun it to fetch the matching standard libraries.
"""

import hashlib
import json
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
TOOLS = REPO_ROOT / "tools"

# The foreign targets code/build.py builds on a Mac.
CROSS_TARGETS = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-gnu"]

# Pinned so every machine builds with the same tools. Tested together with
# rustc 1.98.1; bump deliberately and rebuild all three binaries afterwards.
ZIG_VERSION = "0.16.0"
ZIGBUILD_VERSION = "0.23.4"


def download(url: str, dest: Path) -> None:
    print(f"downloading {url}")
    with urllib.request.urlopen(url) as resp, open(dest, "wb") as out:
        shutil.copyfileobj(resp, out)


def rustc_version() -> str:
    rustc = TOOLS / "rust" / "bin" / "rustc"
    if not rustc.is_file():
        sys.exit(f"No Rust toolchain at {rustc}. Set it up first; see specs/DEVELOPMENT.md.")
    out = subprocess.run([str(rustc), "-vV"], capture_output=True, text=True, check=True).stdout
    for line in out.splitlines():
        if line.startswith("release:"):
            return line.split(":", 1)[1].strip()
    sys.exit("Could not read the rustc version.")


def install_rust_std(version: str, target: str, tmp: Path) -> None:
    rustlib = TOOLS / "rust" / "lib" / "rustlib"
    if (rustlib / target).is_dir():
        print(f"rust-std {target}: present")
        return
    name = f"rust-std-{version}-{target}"
    archive = tmp / f"{name}.tar.xz"
    download(f"https://static.rust-lang.org/dist/{name}.tar.xz", archive)
    with tarfile.open(archive) as tf:
        tf.extractall(tmp, filter="data")
    # The component folder holds lib/rustlib/<target>/...; merge it in.
    src = tmp / name / f"rust-std-{target}" / "lib" / "rustlib" / target
    shutil.copytree(src, rustlib / target)
    print(f"rust-std {target}: installed")


def install_zig(tmp: Path) -> None:
    dest = TOOLS / "zig"
    if (dest / "zig").is_file():
        print("zig: present")
        return
    arch = {"arm64": "aarch64", "x86_64": "x86_64"}[platform.machine()]
    with urllib.request.urlopen("https://ziglang.org/download/index.json") as resp:
        entry = json.load(resp)[ZIG_VERSION][f"{arch}-macos"]
    archive = tmp / "zig.tar.xz"
    download(entry["tarball"], archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != entry["shasum"]:
        sys.exit("zig download does not match its published checksum.")
    with tarfile.open(archive) as tf:
        tf.extractall(tmp, filter="data")
    shutil.move(tmp / f"zig-{arch}-macos-{ZIG_VERSION}", dest)
    print("zig: installed")


def install_zigbuild(tmp: Path) -> None:
    dest = TOOLS / "bin" / "cargo-zigbuild"
    if dest.is_file():
        print("cargo-zigbuild: present")
        return
    arch = {"arm64": "aarch64", "x86_64": "x86_64"}[platform.machine()]
    name = f"cargo-zigbuild-{arch}-apple-darwin"
    archive = tmp / f"{name}.tar.xz"
    download(
        f"https://github.com/rust-cross/cargo-zigbuild/releases/download/v{ZIGBUILD_VERSION}/{name}.tar.xz",
        archive,
    )
    with tarfile.open(archive) as tf:
        tf.extractall(tmp / "zigbuild", filter="data")
    found = next((tmp / "zigbuild").rglob("cargo-zigbuild"))
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(found, dest)
    dest.chmod(0o755)
    print("cargo-zigbuild: installed")


def main() -> int:
    if platform.system() != "Darwin":
        print(
            "Cross-building is set up for macOS hosts only: a Mac can build all three "
            "binaries, while Linux and Windows cannot build the macOS one.",
            file=sys.stderr,
        )
        return 1
    version = rustc_version()
    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        for target in CROSS_TARGETS:
            install_rust_std(version, target, tmp)
        install_zig(tmp)
        install_zigbuild(tmp)
    return 0


if __name__ == "__main__":
    sys.exit(main())
