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
import tarfile
import urllib.request
import zipfile
import stat

def main():
    # Determine paths relative to this script
    script_dir = os.path.dirname(os.path.abspath(__file__))
    project_root = os.path.dirname(script_dir)
    tools_dir = os.path.join(project_root, "tools")
    compiler_dir = os.path.join(tools_dir, "compiler")
    released_dir = os.path.join(project_root, "released")
    code_dir = script_dir

    system = platform.system().lower()
    machine = platform.machine().lower()

    print(f"✓ Platform: {system} ({machine})")

    # Step 1: Ensure Rust is available
    cargo_bin = ensure_rust(compiler_dir, system, machine)

    # Step 2: Verify compiler
    print("✓ Verifying Rust compiler...")
    result = subprocess.run(
        [cargo_bin, "--version"],
        capture_output=True, text=True, encoding='utf-8'
    )
    if result.returncode != 0:
        print(f"✗ Cargo verification failed: {result.stderr}")
        sys.exit(1)
    print(f"  {result.stdout.strip()}")

    rustc_bin = os.path.join(os.path.dirname(cargo_bin), "rustc" + ext(system))
    result = subprocess.run(
        [rustc_bin, "--version"],
        capture_output=True, text=True, encoding='utf-8'
    )
    if result.returncode == 0:
        print(f"  {result.stdout.strip()}")

    # Step 3: Delete everything in released/
    if os.path.exists(released_dir):
        shutil.rmtree(released_dir)
    os.makedirs(released_dir, exist_ok=True)
    print("✓ Cleaned released/ directory")

    # Step 4: Build
    print("✓ Building kitchensync...")
    env = os.environ.copy()
    env["RUSTUP_HOME"] = os.path.join(compiler_dir, "rustup")
    env["CARGO_HOME"] = os.path.join(compiler_dir, "cargo")

    result = subprocess.run(
        [cargo_bin, "build", "--release"],
        cwd=code_dir,
        text=True, encoding='utf-8',
        env=env,
    )
    if result.returncode != 0:
        print("✗ Build failed")
        sys.exit(1)
    print("✓ Build succeeded")

    # Step 5: Copy artifact to released/
    if system == "windows":
        src_binary = os.path.join(code_dir, "target", "release", "kitchensync.exe")
        dst_binary = os.path.join(released_dir, "kitchensync.exe")
    elif system == "darwin":
        src_binary = os.path.join(code_dir, "target", "release", "kitchensync")
        dst_binary = os.path.join(released_dir, "kitchensync.mac")
    else:
        src_binary = os.path.join(code_dir, "target", "release", "kitchensync")
        dst_binary = os.path.join(released_dir, "kitchensync.linux")

    if not os.path.exists(src_binary):
        print(f"✗ Binary not found: {src_binary}")
        sys.exit(1)

    shutil.copy2(src_binary, dst_binary)
    # Make executable
    os.chmod(dst_binary, os.stat(dst_binary).st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)
    size_mb = os.path.getsize(dst_binary) / (1024 * 1024)
    print(f"✓ Copied to {os.path.relpath(dst_binary, project_root)} ({size_mb:.1f} MB)")

    print("✓ Build complete!")


def ext(system):
    return ".exe" if system == "windows" else ""


def ensure_rust(compiler_dir, system, machine):
    """Ensure Rust toolchain is available in compiler_dir."""
    cargo_home = os.path.join(compiler_dir, "cargo")
    rustup_home = os.path.join(compiler_dir, "rustup")
    cargo_bin = os.path.join(cargo_home, "bin", "cargo" + ext(system))

    if os.path.exists(cargo_bin):
        print("✓ Rust compiler found")
        return cargo_bin

    print("✓ Downloading Rust toolchain...")
    os.makedirs(compiler_dir, exist_ok=True)

    env = os.environ.copy()
    env["RUSTUP_HOME"] = rustup_home
    env["CARGO_HOME"] = cargo_home

    if system == "windows":
        # Download rustup-init.exe
        rustup_url = "https://win.rustup.rs/x86_64"
        rustup_init = os.path.join(compiler_dir, "rustup-init.exe")
        download_file(rustup_url, rustup_init)
        subprocess.run(
            [rustup_init, "-y", "--default-toolchain", "stable-x86_64-pc-windows-gnu", "--no-modify-path"],
            env=env, text=True, encoding='utf-8',
        )
    else:
        # Download rustup-init shell script
        rustup_url = "https://sh.rustup.rs"
        rustup_script = os.path.join(compiler_dir, "rustup-init.sh")
        download_file(rustup_url, rustup_script)
        os.chmod(rustup_script, 0o755)

        # Determine target triple
        if system == "darwin":
            if "arm" in machine or "aarch64" in machine:
                target = "stable-aarch64-apple-darwin"
            else:
                target = "stable-x86_64-apple-darwin"
        else:
            if "aarch64" in machine or "arm" in machine:
                target = "stable-aarch64-unknown-linux-gnu"
            else:
                target = "stable-x86_64-unknown-linux-gnu"

        result = subprocess.run(
            ["sh", rustup_script, "-y", "--default-toolchain", target, "--no-modify-path"],
            env=env, text=True, encoding='utf-8',
        )
        if result.returncode != 0:
            print("✗ Rust installation failed")
            sys.exit(1)

    if not os.path.exists(cargo_bin):
        print(f"✗ Cargo not found after installation: {cargo_bin}")
        sys.exit(1)

    print("✓ Rust toolchain installed")
    return cargo_bin


def download_file(url, dest):
    """Download a file from url to dest."""
    print(f"  Downloading {url}...")
    urllib.request.urlretrieve(url, dest)


if __name__ == "__main__":
    main()
