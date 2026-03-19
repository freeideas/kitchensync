#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "requests",
# ]
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
import stat

def main():
    os.chdir(os.path.dirname(os.path.abspath(__file__)))
    os.chdir("..")  # project root

    system = platform.system().lower()
    machine = platform.machine().lower()

    print(f"✓ Platform: {system} {machine}")

    # Ensure Rust toolchain
    ensure_rust_toolchain(system, machine)

    # Clean released/
    released_dir = os.path.join(".", "released")
    if os.path.exists(released_dir):
        shutil.rmtree(released_dir)
    os.makedirs(released_dir, exist_ok=True)
    print("✓ Cleaned released/")

    # Build
    cargo = find_cargo(system)
    print(f"✓ Using cargo: {cargo}")

    env = os.environ.copy()
    rustup_home = os.path.abspath(os.path.join(".", "tools", "compiler", "rustup"))
    cargo_home = os.path.abspath(os.path.join(".", "tools", "compiler", "cargo"))
    env["RUSTUP_HOME"] = rustup_home
    env["CARGO_HOME"] = cargo_home
    env["PATH"] = os.path.join(cargo_home, "bin") + os.pathsep + env.get("PATH", "")

    print("Building kitchensync (release)...")
    result = subprocess.run(
        [cargo, "build", "--release", "--manifest-path", os.path.join(".", "code", "Cargo.toml")],
        env=env,
        text=True,
        encoding='utf-8',
        capture_output=True,
    )
    if result.returncode != 0:
        print("Build FAILED:")
        print(result.stderr)
        print(result.stdout)
        sys.exit(1)
    print("✓ Build succeeded")

    # Copy artifact to released/
    target_dir = os.path.join(".", "code", "target", "release")
    if system == "windows":
        src_name = "kitchensync.exe"
        dst_name = "kitchensync.exe"
    elif system == "darwin":
        src_name = "kitchensync"
        dst_name = "kitchensync.mac"
    else:
        src_name = "kitchensync"
        dst_name = "kitchensync.linux"

    src_path = os.path.join(target_dir, src_name)
    dst_path = os.path.join(released_dir, dst_name)

    if not os.path.exists(src_path):
        print(f"ERROR: Expected binary not found: {src_path}")
        sys.exit(1)

    shutil.copy2(src_path, dst_path)
    # Make executable
    os.chmod(dst_path, os.stat(dst_path).st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)
    print(f"✓ Copied {dst_name} to released/")
    print("✓ Build complete!")


def find_cargo(system):
    if system == "windows":
        cargo = os.path.abspath(os.path.join(".", "tools", "compiler", "cargo", "bin", "cargo.exe"))
    else:
        cargo = os.path.abspath(os.path.join(".", "tools", "compiler", "cargo", "bin", "cargo"))
    if os.path.exists(cargo):
        return cargo
    # Fallback: check PATH
    which = shutil.which("cargo")
    if which:
        return which
    raise RuntimeError("Cargo not found. Run ensure_rust_toolchain first.")


def ensure_rust_toolchain(system, machine):
    cargo_bin = os.path.join(".", "tools", "compiler", "cargo", "bin")
    cargo_exe = "cargo.exe" if system == "windows" else "cargo"
    if os.path.exists(os.path.join(cargo_bin, cargo_exe)):
        print("✓ Rust toolchain already installed")
        # Verify it works
        verify_cargo(os.path.join(cargo_bin, cargo_exe), system)
        return

    print("Installing Rust toolchain to ./tools/compiler/...")
    os.makedirs(os.path.join(".", "tools", "compiler"), exist_ok=True)

    rustup_home = os.path.abspath(os.path.join(".", "tools", "compiler", "rustup"))
    cargo_home = os.path.abspath(os.path.join(".", "tools", "compiler", "cargo"))

    env = os.environ.copy()
    env["RUSTUP_HOME"] = rustup_home
    env["CARGO_HOME"] = cargo_home

    if system == "windows":
        download_rustup_windows(env)
    else:
        download_rustup_unix(system, machine, env)

    verify_cargo(os.path.join(cargo_bin, cargo_exe), system)


def download_rustup_unix(system, machine, env):
    import requests

    # Determine target triple
    if system == "darwin":
        if machine in ("arm64", "aarch64"):
            target = "aarch64-apple-darwin"
        else:
            target = "x86_64-apple-darwin"
    else:  # linux
        if machine in ("aarch64", "arm64"):
            target = "aarch64-unknown-linux-gnu"
        else:
            target = "x86_64-unknown-linux-gnu"

    url = f"https://static.rust-lang.org/rustup/dist/{target}/rustup-init"
    tmp_dir = os.path.join(".", "tmp")
    os.makedirs(tmp_dir, exist_ok=True)
    rustup_init = os.path.join(tmp_dir, "rustup-init")

    print(f"  Downloading rustup-init for {target}...")
    resp = requests.get(url, stream=True)
    resp.raise_for_status()
    with open(rustup_init, "wb") as f:
        for chunk in resp.iter_content(chunk_size=8192):
            f.write(chunk)

    os.chmod(rustup_init, 0o755)

    print("  Running rustup-init...")
    result = subprocess.run(
        [rustup_init, "-y", "--default-toolchain", "stable", "--no-modify-path",
         "--profile", "minimal"],
        env=env,
        text=True,
        encoding='utf-8',
        capture_output=True,
    )
    if result.returncode != 0:
        print(f"rustup-init failed:\n{result.stderr}\n{result.stdout}")
        sys.exit(1)

    print("✓ Rust toolchain installed")


def download_rustup_windows(env):
    import requests

    url = "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-gnu/rustup-init.exe"
    tmp_dir = os.path.join(".", "tmp")
    os.makedirs(tmp_dir, exist_ok=True)
    rustup_init = os.path.join(tmp_dir, "rustup-init.exe")

    print("  Downloading rustup-init.exe for windows-gnu...")
    resp = requests.get(url, stream=True)
    resp.raise_for_status()
    with open(rustup_init, "wb") as f:
        for chunk in resp.iter_content(chunk_size=8192):
            f.write(chunk)

    print("  Running rustup-init.exe...")
    result = subprocess.run(
        [rustup_init, "-y", "--default-toolchain", "stable-x86_64-pc-windows-gnu",
         "--no-modify-path", "--profile", "minimal"],
        env=env,
        text=True,
        encoding='utf-8',
        capture_output=True,
    )
    if result.returncode != 0:
        print(f"rustup-init failed:\n{result.stderr}\n{result.stdout}")
        sys.exit(1)

    print("✓ Rust toolchain installed (windows-gnu)")


def verify_cargo(cargo_path, system):
    env = os.environ.copy()
    rustup_home = os.path.abspath(os.path.join(".", "tools", "compiler", "rustup"))
    cargo_home = os.path.abspath(os.path.join(".", "tools", "compiler", "cargo"))
    env["RUSTUP_HOME"] = rustup_home
    env["CARGO_HOME"] = cargo_home
    env["PATH"] = os.path.join(cargo_home, "bin") + os.pathsep + env.get("PATH", "")

    result = subprocess.run(
        [cargo_path, "--version"],
        env=env,
        text=True,
        encoding='utf-8',
        capture_output=True,
    )
    if result.returncode == 0:
        print(f"✓ Cargo version: {result.stdout.strip()}")
    else:
        print(f"WARNING: cargo --version failed: {result.stderr}")


if __name__ == "__main__":
    main()
