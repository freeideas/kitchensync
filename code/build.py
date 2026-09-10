#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Builds the KitchenSync release binary using the portable toolchain under
./tools/ and copies it into ./released/ under the right platform name.

Run as: uv run code/build.py [--test]

See specs/DEVELOPMENT.md for the toolchain and released/ layout this script
assumes.

With --test, also runs the end-to-end test suite (tests/run.py) afterward and
exits with its status.
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


def main(argv: list[str]) -> int:
    run_tests = "--test" in argv

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
    env["CARGO_HOME"] = str(cargo_home)

    manifest = REPO_ROOT / "code" / "Cargo.toml"
    build = subprocess.run(
        [str(cargo_exe), "build", "--release", "--manifest-path", str(manifest)],
        env=env,
    )
    if build.returncode != 0:
        print(f"cargo build failed with exit code {build.returncode}", file=sys.stderr)
        return build.returncode

    built = REPO_ROOT / "code" / "target" / "release" / cargo_build_output_name()
    if not built.is_file():
        print(
            f"cargo build reported success but no output was found at {built}",
            file=sys.stderr,
        )
        return 1

    released_dir = REPO_ROOT / "released"
    released_dir.mkdir(parents=True, exist_ok=True)
    dest = released_dir / released_binary_name()
    # Replace, never overwrite in place: macOS refuses to run a signed binary
    # whose bytes changed underneath it, and a running copy keeps working.
    if dest.exists():
        dest.unlink()
    shutil.copy2(built, dest)

    if platform.system() != "Windows":
        mode = dest.stat().st_mode
        dest.chmod(mode | 0o111)

    size_bytes = dest.stat().st_size
    size_mb = size_bytes / (1024 * 1024)
    print(f"{dest} ({size_bytes} bytes, {size_mb:.2f} MB)")

    if run_tests:
        tests_script = REPO_ROOT / "tests" / "run.py"
        test_run = subprocess.run(["uv", "run", str(tests_script)])
        return test_run.returncode

    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
