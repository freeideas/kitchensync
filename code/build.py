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
import requests

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(SCRIPT_DIR)
TOOLS_DIR = os.path.join(PROJECT_ROOT, "tools")
COMPILER_DIR = os.path.join(TOOLS_DIR, "compiler")
RUSTUP_HOME = os.path.join(COMPILER_DIR, "rustup")
CARGO_HOME = os.path.join(COMPILER_DIR, "cargo")
RELEASED_DIR = os.path.join(PROJECT_ROOT, "released")
CODE_DIR = SCRIPT_DIR

# Cache for MSVC environment variables (populated once on first use)
_msvc_env_cache = None


def _find_vcvarsall():
    """Find vcvarsall.bat from VS Build Tools or VS Community."""
    candidates = [
        os.path.join(os.environ.get("ProgramFiles(x86)", "C:\\Program Files (x86)"),
                     "Microsoft Visual Studio", "2022", "BuildTools",
                     "VC", "Auxiliary", "Build", "vcvarsall.bat"),
        os.path.join(os.environ.get("ProgramFiles(x86)", "C:\\Program Files (x86)"),
                     "Microsoft Visual Studio", "2022", "Community",
                     "VC", "Auxiliary", "Build", "vcvarsall.bat"),
        os.path.join(os.environ.get("ProgramFiles", "C:\\Program Files"),
                     "Microsoft Visual Studio", "2022", "BuildTools",
                     "VC", "Auxiliary", "Build", "vcvarsall.bat"),
        os.path.join(os.environ.get("ProgramFiles", "C:\\Program Files"),
                     "Microsoft Visual Studio", "2022", "Community",
                     "VC", "Auxiliary", "Build", "vcvarsall.bat"),
    ]
    for path in candidates:
        if os.path.isfile(path):
            return path
    return None


def _get_msvc_env():
    """Capture environment variables from vcvarsall.bat x64. Returns dict or None."""
    global _msvc_env_cache
    if _msvc_env_cache is not None:
        return _msvc_env_cache

    vcvarsall = _find_vcvarsall()
    if not vcvarsall:
        return None

    # Run vcvarsall.bat and dump the resulting environment
    bat_cmd = f'call "{vcvarsall}" x64 >nul 2>&1 && set'
    result = subprocess.run(
        ["cmd.exe", "/c", bat_cmd],
        capture_output=True, text=True, encoding='utf-8',
    )
    if result.returncode != 0:
        return None

    msvc_env = {}
    for line in result.stdout.splitlines():
        if '=' in line:
            key, _, value = line.partition('=')
            msvc_env[key] = value

    _msvc_env_cache = msvc_env
    print(f"  ✓ Loaded MSVC environment from {os.path.basename(os.path.dirname(os.path.dirname(os.path.dirname(vcvarsall))))}")
    return msvc_env


def run(cmd, **kwargs):
    """Run a command with Rust environment."""
    env = os.environ.copy()

    # On Windows, load MSVC environment (PATH, LIB, INCLUDE, etc.) so that
    # Rust can find link.exe and Windows SDK libs instead of picking up the
    # GNU link.exe from Git Bash's /usr/bin/.
    if platform.system().lower() == "windows":
        msvc_env = _get_msvc_env()
        if msvc_env:
            # Merge MSVC env — keys like PATH, LIB, INCLUDE, LIBPATH are critical
            for key in ("PATH", "LIB", "INCLUDE", "LIBPATH", "WINDOWSSDKDIR",
                        "WINDOWSSDKVERSION", "VCTOOLSINSTALLDIR", "UCRTCONTENTSDIR"):
                if key in msvc_env:
                    env[key] = msvc_env[key]

    env["RUSTUP_HOME"] = RUSTUP_HOME
    env["CARGO_HOME"] = CARGO_HOME
    cargo_bin = os.path.join(CARGO_HOME, "bin")

    # On Windows, add Strawberry Perl to PATH for OpenSSL build
    strawberry_perl = os.path.join(TOOLS_DIR, "strawberry", "perl", "bin")
    strawberry_c = os.path.join(TOOLS_DIR, "strawberry", "c", "bin")
    extra_paths = []
    if os.path.isdir(strawberry_perl):
        extra_paths.extend([strawberry_perl, strawberry_c])

    env["PATH"] = os.pathsep.join([cargo_bin] + extra_paths + [env.get("PATH", "")])

    print(f"  ✓ Running: {' '.join(cmd) if isinstance(cmd, list) else cmd}")
    result = subprocess.run(
        cmd,
        env=env,
        text=True,
        encoding='utf-8',
        capture_output=True,
        **kwargs,
    )
    if result.returncode != 0:
        print(f"  ✗ Command failed (exit {result.returncode})")
        if result.stdout:
            # Print last 30 lines of stdout
            lines = result.stdout.strip().split('\n')
            for line in lines[-30:]:
                print(f"    {line}")
        if result.stderr:
            lines = result.stderr.strip().split('\n')
            for line in lines[-30:]:
                print(f"    {line}")
    return result


def download_file(url, dest):
    """Download a file from URL to dest."""
    print(f"  ✓ Downloading {url}")
    resp = requests.get(url, stream=True, timeout=300)
    resp.raise_for_status()
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with open(dest, 'wb') as f:
        for chunk in resp.iter_content(chunk_size=8192):
            f.write(chunk)
    print(f"  ✓ Saved to {dest}")


def ensure_rust():
    """Ensure Rust toolchain is installed in ./tools/compiler/."""
    cargo_bin = os.path.join(CARGO_HOME, "bin", "cargo")
    rustc_bin = os.path.join(CARGO_HOME, "bin", "rustc")
    rustup_bin = os.path.join(CARGO_HOME, "bin", "rustup")

    # Check if executables exist (on Windows, add .exe)
    if platform.system().lower() == "windows":
        cargo_bin += ".exe"
        rustc_bin += ".exe"
        rustup_bin += ".exe"

    if os.path.isfile(cargo_bin) and os.path.isfile(rustc_bin):
        result = run([rustc_bin, "--version"])
        if result.returncode == 0:
            print(f"  ✓ Rust already installed: {result.stdout.strip()}")
            return
        # Rustc exists but --version failed, may need to set default toolchain
        if os.path.isfile(rustup_bin):
            print("  • Setting default toolchain...")
            default_result = run([rustup_bin, "default", "stable"])
            if default_result.returncode == 0:
                result = run([rustc_bin, "--version"])
                if result.returncode == 0:
                    print(f"  ✓ Rust already installed: {result.stdout.strip()}")
                    return

    print("• Installing Rust toolchain...")
    os.makedirs(COMPILER_DIR, exist_ok=True)

    system = platform.system().lower()
    if system == "linux":
        rustup_url = "https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init"
        rustup_init = os.path.join(COMPILER_DIR, "rustup-init")
    elif system == "darwin":
        rustup_url = "https://static.rust-lang.org/rustup/dist/x86_64-apple-darwin/rustup-init"
        rustup_init = os.path.join(COMPILER_DIR, "rustup-init")
    else:
        rustup_url = "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"
        rustup_init = os.path.join(COMPILER_DIR, "rustup-init.exe")

    download_file(rustup_url, rustup_init)
    os.chmod(rustup_init, 0o755)

    result = run([rustup_init, "-y", "--default-toolchain", "stable", "--no-modify-path"])
    if result.returncode != 0:
        print("  ✗ Failed to install Rust")
        sys.exit(1)

    result = run([os.path.join(CARGO_HOME, "bin", "rustc"), "--version"])
    print(f"  ✓ Rust installed: {result.stdout.strip()}")


def detect_platform():
    """Detect current platform and return the native target triple."""
    system = platform.system().lower()
    machine = platform.machine().lower()

    if system == "linux":
        return "x86_64-unknown-linux-gnu" if "x86_64" in machine else f"{machine}-unknown-linux-gnu"
    elif system == "darwin":
        return "x86_64-apple-darwin" if "x86_64" in machine else "aarch64-apple-darwin"
    elif system == "windows":
        return "x86_64-pc-windows-msvc"
    return "x86_64-unknown-linux-gnu"


def build_native(target):
    """Build for the native platform."""
    cargo = os.path.join(CARGO_HOME, "bin", "cargo")
    cmd = [cargo, "build", "--release", "--target", target]

    print(f"• Building for {target} (native)...")
    result = run(cmd, cwd=CODE_DIR, timeout=1800)  # 30 minutes for vendored OpenSSL build

    if result.returncode != 0:
        print(f"  ✗ Build failed for {target}")
        return False

    # Find the binary
    if "windows" in target:
        binary_name = "kitchensync.exe"
    else:
        binary_name = "kitchensync"

    binary_path = os.path.join(CODE_DIR, "target", target, "release", binary_name)
    if not os.path.isfile(binary_path):
        print(f"  ✗ Binary not found at {binary_path}")
        return False

    # Determine output name
    system = platform.system().lower()
    if system == "linux":
        output_name = "kitchensync.linux"
    elif system == "darwin":
        output_name = "kitchensync.mac"
    else:
        output_name = "kitchensync.exe"

    dest = os.path.join(RELEASED_DIR, output_name)
    shutil.copy2(binary_path, dest)
    if not output_name.endswith(".exe"):
        os.chmod(dest, 0o755)

    size_mb = os.path.getsize(dest) / (1024 * 1024)
    print(f"  ✓ Built {output_name} ({size_mb:.1f} MB)")
    return True


def try_cross_compile(target, output_name):
    """Attempt cross-compilation for a target. Returns True on success."""
    rustup = os.path.join(CARGO_HOME, "bin", "rustup")
    cargo = os.path.join(CARGO_HOME, "bin", "cargo")

    # Add target
    result = run([rustup, "target", "add", target])
    if result.returncode != 0:
        print(f"  ✗ Could not add target {target}")
        return False

    print(f"• Cross-compiling for {target}...")

    # For Windows cross-compilation from Linux, we need MinGW
    env_extra = {}
    if "windows" in target and platform.system().lower() == "linux":
        # Check if MinGW is available
        mingw_gcc = shutil.which("x86_64-w64-mingw32-gcc")
        if mingw_gcc:
            print(f"  ✓ Found MinGW at {mingw_gcc}")
        else:
            print(f"  ✗ MinGW not found. Install mingw-w64 for Windows cross-compilation.")
            return False

    result = run([cargo, "build", "--release", "--target", target], cwd=CODE_DIR, timeout=600)

    if result.returncode != 0:
        print(f"  ✗ Cross-compilation failed for {target}")
        return False

    if "windows" in target:
        binary_name = "kitchensync.exe"
    else:
        binary_name = "kitchensync"

    binary_path = os.path.join(CODE_DIR, "target", target, "release", binary_name)
    if not os.path.isfile(binary_path):
        print(f"  ✗ Binary not found at {binary_path}")
        return False

    dest = os.path.join(RELEASED_DIR, output_name)
    shutil.copy2(binary_path, dest)
    if not output_name.endswith(".exe"):
        os.chmod(dest, 0o755)

    size_mb = os.path.getsize(dest) / (1024 * 1024)
    print(f"  ✓ Built {output_name} ({size_mb:.1f} MB)")
    return True


def main():
    print("=" * 60)
    print("KitchenSync Build")
    print("=" * 60)

    # Step 0: Delete only current platform's binary (preserve other platforms)
    os.makedirs(RELEASED_DIR, exist_ok=True)
    system = platform.system().lower()
    if system == "linux":
        current_binary = "kitchensync.linux"
    elif system == "darwin":
        current_binary = "kitchensync.mac"
    else:
        current_binary = "kitchensync.exe"
    current_binary_path = os.path.join(RELEASED_DIR, current_binary)
    if os.path.isfile(current_binary_path):
        os.remove(current_binary_path)
        print(f"  ✓ Removed old {current_binary}")
    os.makedirs(TOOLS_DIR, exist_ok=True)

    # Step 1: Ensure Rust
    print("\n[1/2] Checking Rust toolchain...")
    ensure_rust()

    # Step 2: Build native
    native_target = detect_platform()
    print(f"\n[2/2] Building native binary ({native_target})...")
    native_ok = build_native(native_target)

    if not native_ok:
        print("\n✗ Native build failed!")
        sys.exit(1)

    # Summary
    print("\n" + "=" * 60)
    print("Build Summary")
    print("=" * 60)
    print(f"  ✓ {native_target} (native)")

    print(f"\nArtifacts in {RELEASED_DIR}:")
    if os.path.isdir(RELEASED_DIR):
        for f in sorted(os.listdir(RELEASED_DIR)):
            path = os.path.join(RELEASED_DIR, f)
            if os.path.isfile(path):
                size_mb = os.path.getsize(path) / (1024 * 1024)
                print(f"  • {f} ({size_mb:.1f} MB)")

    print("\n✓ Build complete!")


if __name__ == "__main__":
    main()
