#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

import sys
# Fix Windows console encoding for Unicode characters
if sys.stdout.encoding != 'utf-8':
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')

import os
import platform
import subprocess
import shutil

def main():
    # Determine paths
    code_dir = os.path.dirname(os.path.abspath(__file__))
    project_dir = os.path.dirname(code_dir)
    released_dir = os.path.join(project_dir, "released")
    go_bin = os.path.join(project_dir, "tools", "compiler", "go", "bin", "go")

    # Determine current platform
    system = platform.system().lower()
    if system == "linux":
        current_binary = "kitchensync.linux"
        current_goos = "linux"
    elif system == "darwin":
        current_binary = "kitchensync.mac"
        current_goos = "darwin"
    elif system == "windows":
        current_binary = "kitchensync.exe"
        current_goos = "windows"
    else:
        print(f"✗ Unsupported platform: {system}")
        return 1

    machine = platform.machine().lower()
    if machine in ("x86_64", "amd64"):
        current_goarch = "amd64"
    elif machine in ("aarch64", "arm64"):
        current_goarch = "arm64"
    else:
        current_goarch = "amd64"

    # Ensure released directory exists
    os.makedirs(released_dir, exist_ok=True)

    # Step 1: Delete only current platform binary (preserve others)
    current_binary_path = os.path.join(released_dir, current_binary)
    if os.path.exists(current_binary_path):
        os.remove(current_binary_path)
        print(f"✓ Removed old {current_binary}")

    # Step 2: Build all platform binaries
    targets = [
        ("linux", "amd64", "kitchensync.linux"),
        ("windows", "amd64", "kitchensync.exe"),
        ("darwin", "amd64", "kitchensync.mac"),
    ]

    env = os.environ.copy()
    env["GOPATH"] = os.path.join(project_dir, "tools", "gopath")
    # CGO is needed for sqlite but modernc.org/sqlite is pure Go
    env["CGO_ENABLED"] = "0"

    for goos, goarch, binary_name in targets:
        output_path = os.path.join(released_dir, binary_name)

        # Skip if binary already exists from another platform's build
        # (only rebuild current platform's binary, cross-compile the rest)
        if os.path.exists(output_path) and binary_name != current_binary:
            print(f"• Skipping {binary_name} (already exists)")
            continue

        print(f"• Building {binary_name} (GOOS={goos} GOARCH={goarch})...")

        build_env = env.copy()
        build_env["GOOS"] = goos
        build_env["GOARCH"] = goarch

        result = subprocess.run(
            [go_bin, "build", "-o", output_path, "./cmd/kitchensync"],
            cwd=code_dir,
            env=build_env,
            text=True,
            encoding='utf-8',
            capture_output=True,
        )

        if result.returncode != 0:
            print(f"✗ Build failed for {binary_name}:")
            print(result.stderr)
            return 1

        print(f"✓ Built {binary_name}")

    # Verify artifacts
    print()
    print("Released artifacts:")
    for f in sorted(os.listdir(released_dir)):
        fpath = os.path.join(released_dir, f)
        size_mb = os.path.getsize(fpath) / (1024 * 1024)
        print(f"  {f} ({size_mb:.1f} MB)")

    print()
    print("✓ Build complete")
    return 0


if __name__ == "__main__":
    sys.exit(main())
