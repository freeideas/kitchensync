#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Builds the KitchenSync release binaries using the portable toolchain under
./tools/ and copies them into ./released/ under the right platform names.

Run as: uv run code/build.py [--native] [--test]

On macOS it builds all three binaries: the Mac one natively, and the Linux and
Windows ones by cross-compiling with Zig (set that up once with
`uv run code/setup-cross-tools.py`). With --native, or on Linux or Windows, it
builds only the binary for the machine it runs on.

See specs/DEVELOPMENT.md for the toolchain and released/ layout this script
assumes.

With --test, also runs the end-to-end test suite (tests/run.py) afterward and
exits with its status. The tests exercise only this machine's own binary.
"""

import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

# This file lives at <repo root>/code/build.py.
REPO_ROOT = Path(__file__).resolve().parent.parent


def released_binary_name() -> str:
    system = platform.system()
    if system == "Windows":
        return "kitchensync.exe"
    if system == "Darwin":
        return "kitchensync.mac"
    if system == "Linux":
        return "kitchensync.linux"
    print(f"Unsupported platform: {system}", file=sys.stderr)
    sys.exit(1)


def cargo_build_output_name() -> str:
    return "kitchensync.exe" if platform.system() == "Windows" else "kitchensync"


# (Rust target, file cargo writes, name under released/) for each foreign
# binary a Mac cross-compiles.
CROSS_TARGETS = [
    ("x86_64-unknown-linux-gnu", "kitchensync", "kitchensync.linux"),
    ("x86_64-pc-windows-gnu", "kitchensync.exe", "kitchensync.exe"),
]


def release(built: Path, name: str) -> None:
    if not built.is_file():
        sys.exit(f"cargo build reported success but no output was found at {built}")
    released_dir = REPO_ROOT / "released"
    released_dir.mkdir(parents=True, exist_ok=True)
    dest = released_dir / name
    # Replace, never overwrite in place: macOS refuses to run a signed binary
    # whose bytes changed underneath it, and a running copy keeps working.
    if dest.exists():
        dest.unlink()
    shutil.copy2(built, dest)
    if not name.endswith(".exe"):
        dest.chmod(dest.stat().st_mode | 0o111)
    size_bytes = dest.stat().st_size
    print(f"{dest} ({size_bytes} bytes, {size_bytes / (1024 * 1024):.2f} MB)")


def main(argv: list[str]) -> int:
    run_tests = "--test" in argv
    cross = platform.system() == "Darwin" and "--native" not in argv

    tools_bin = REPO_ROOT / "tools" / "rust" / "bin"
    if not tools_bin.is_dir():
        print(
            "The portable Rust toolchain is missing.\n"
            f"Expected a 'bin' directory at {tools_bin}, but it is not there.\n"
            "Set up the local toolchain under ./tools/ before building - see "
            "specs/DEVELOPMENT.md for how.",
            file=sys.stderr,
        )
        return 1

    cargo_name = "cargo.exe" if platform.system() == "Windows" else "cargo"
    cargo_exe = tools_bin / cargo_name
    if not cargo_exe.is_file():
        print(
            f"Expected '{cargo_name}' under {tools_bin}, but it is not there.\n"
            "Set up the local toolchain under ./tools/ before building - see "
            "specs/DEVELOPMENT.md for how.",
            file=sys.stderr,
        )
        return 1

    cargo_home = REPO_ROOT / "tools" / "cargo-home"
    env = os.environ.copy()
    env["PATH"] = f"{tools_bin}{os.pathsep}{env.get('PATH', '')}"
    # russh's crypto backend (aws-lc-sys) needs NASM when built with MSVC.
    # A portable copy lives under ./tools/nasm/ (see specs/DEVELOPMENT.md).
    nasm_dir = REPO_ROOT / "tools" / "nasm"
    if platform.system() == "Windows" and nasm_dir.is_dir():
        env["PATH"] = f"{nasm_dir}{os.pathsep}{env['PATH']}"
    env["CARGO_HOME"] = str(cargo_home)

    zigbuild = REPO_ROOT / "tools" / "bin" / "cargo-zigbuild"
    zig_dir = REPO_ROOT / "tools" / "zig"
    if cross and not (zigbuild.is_file() and (zig_dir / "zig").is_file()):
        print(
            "The cross-compiling tools are missing from ./tools/.\n"
            "Run `uv run code/setup-cross-tools.py` once, or pass --native to "
            "build only the macOS binary.",
            file=sys.stderr,
        )
        return 1

    manifest = REPO_ROOT / "code" / "Cargo.toml"
    build = subprocess.run(
        [str(cargo_exe), "build", "--release", "--manifest-path", str(manifest)],
        env=env,
    )
    if build.returncode != 0:
        print(f"cargo build failed with exit code {build.returncode}", file=sys.stderr)
        return build.returncode

    release(REPO_ROOT / "code" / "target" / "release" / cargo_build_output_name(), released_binary_name())

    if cross:
        cross_env = env.copy()
        cross_env["PATH"] = f"{zigbuild.parent}{os.pathsep}{zig_dir}{os.pathsep}{env['PATH']}"
        # aws-lc-sys (russh's crypto) needs NASM for x86_64 Windows; this makes
        # it use the pre-assembled objects it ships with instead.
        cross_env["AWS_LC_SYS_PREBUILT_NASM"] = "1"
        for target, output, name in CROSS_TARGETS:
            build = subprocess.run(
                [str(cargo_exe), "zigbuild", "--release", "--manifest-path", str(manifest),
                 "--target", target],
                env=cross_env,
            )
            if build.returncode != 0:
                print(f"cargo zigbuild for {target} failed with exit code {build.returncode}",
                      file=sys.stderr)
                return build.returncode
            release(REPO_ROOT / "code" / "target" / target / "release" / output, name)

    if run_tests:
        tests_script = REPO_ROOT / "tests" / "run.py"
        test_run = subprocess.run(["uv", "run", str(tests_script)])
        return test_run.returncode

    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
