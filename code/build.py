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
import shutil
import subprocess

def main():
    script_dir = os.path.dirname(os.path.abspath(__file__))
    project_root = os.path.dirname(script_dir)
    code_dir = script_dir
    released_dir = os.path.join(project_root, "released")
    tools_dir = os.path.join(project_root, "tools", "compiler")

    # Determine platform
    system = platform.system().lower()
    if system == "linux":
        binary_name = "kitchensync.linux"
    elif system == "windows":
        binary_name = "kitchensync.exe"
    elif system == "darwin":
        binary_name = "kitchensync.mac"
    else:
        print(f"✗ Unsupported platform: {system}")
        sys.exit(1)

    # Set up Rust toolchain paths
    rustup_home = os.path.join(tools_dir, "rustup")
    cargo_home = os.path.join(tools_dir, "cargo")
    cargo_bin = os.path.join(cargo_home, "bin")

    if system == "windows":
        cargo_exe = os.path.join(cargo_bin, "cargo.exe")
    else:
        cargo_exe = os.path.join(cargo_bin, "cargo")

    if not os.path.exists(cargo_exe):
        print(f"✗ Cargo not found at {cargo_exe}")
        print("  Run rustup installer first to set up the toolchain in ./tools/compiler/")
        sys.exit(1)

    # Verify toolchain
    env = os.environ.copy()
    env["RUSTUP_HOME"] = rustup_home
    env["CARGO_HOME"] = cargo_home
    env["PATH"] = cargo_bin + os.pathsep + env.get("PATH", "")

    print("✓ Using Rust toolchain from ./tools/compiler/")
    result = subprocess.run(
        [cargo_exe, "--version"],
        capture_output=True, text=True, encoding='utf-8', env=env
    )
    if result.returncode != 0:
        print(f"✗ Cargo version check failed: {result.stderr}")
        sys.exit(1)
    print(f"  {result.stdout.strip()}")

    # Step 1: Delete everything in ./released/
    print("\n• Cleaning ./released/")
    if os.path.exists(released_dir):
        shutil.rmtree(released_dir)
    os.makedirs(released_dir, exist_ok=True)

    # Step 2: Build the binary
    print(f"\n• Building kitchensync for {system}...")
    result = subprocess.run(
        [cargo_exe, "build", "--release"],
        cwd=code_dir,
        env=env,
        text=True,
        encoding='utf-8',
    )
    if result.returncode != 0:
        print(f"\n✗ Build failed (exit code {result.returncode})")
        sys.exit(1)

    # Step 3: Copy to ./released/ with platform-appropriate name
    if system == "windows":
        built_binary = os.path.join(code_dir, "target", "release", "kitchensync.exe")
    else:
        built_binary = os.path.join(code_dir, "target", "release", "kitchensync")

    if not os.path.exists(built_binary):
        print(f"✗ Built binary not found at {built_binary}")
        sys.exit(1)

    dest = os.path.join(released_dir, binary_name)
    shutil.copy2(built_binary, dest)

    # Make executable on Unix
    if system != "windows":
        os.chmod(dest, 0o755)

    size_mb = os.path.getsize(dest) / (1024 * 1024)
    print(f"\n✓ Build complete: ./released/{binary_name} ({size_mb:.1f} MB)")

    # Verify the binary runs
    print("\n• Verifying binary...")
    result = subprocess.run(
        [dest, "--help"],
        capture_output=True, text=True, encoding='utf-8',
        timeout=10
    )
    if result.returncode != 0:
        print(f"✗ Binary verification failed (exit code {result.returncode})")
        print(result.stderr)
        sys.exit(1)
    print("✓ Binary runs successfully (--help exits 0)")


if __name__ == "__main__":
    main()
