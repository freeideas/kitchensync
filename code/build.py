#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "httpx",
# ]
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
import tarfile
import zipfile
import tempfile

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(SCRIPT_DIR)
CODE_DIR = SCRIPT_DIR
RELEASED_DIR = os.path.join(PROJECT_ROOT, "released")
TOOLS_DIR = os.path.join(PROJECT_ROOT, "tools", "compiler")
GO_DIR = os.path.join(TOOLS_DIR, "go")

GO_VERSION = "1.22.10"

PLATFORMS = {
    "linux":   {"goos": "linux",   "goarch": "amd64", "binary": "kitchensync.linux"},
    "windows": {"goos": "windows", "goarch": "amd64", "binary": "kitchensync.exe"},
    "darwin":  {"goos": "darwin",  "goarch": "arm64", "binary": "kitchensync.mac"},
}


def current_platform():
    s = platform.system().lower()
    if s == "windows":
        return "windows"
    elif s == "darwin":
        return "darwin"
    else:
        return "linux"


def go_binary():
    if platform.system().lower() == "windows":
        return os.path.join(GO_DIR, "bin", "go.exe")
    return os.path.join(GO_DIR, "bin", "go")


def download_go():
    if os.path.isfile(go_binary()):
        print(f"✓ Go already provisioned at {go_binary()}")
        return

    print(f"Downloading Go {GO_VERSION}...")
    os.makedirs(TOOLS_DIR, exist_ok=True)

    sys_name = platform.system().lower()
    if sys_name == "windows":
        archive_name = f"go{GO_VERSION}.windows-amd64.zip"
    elif sys_name == "darwin":
        machine = platform.machine()
        arch = "arm64" if machine == "arm64" else "amd64"
        archive_name = f"go{GO_VERSION}.darwin-{arch}.tar.gz"
    else:
        archive_name = f"go{GO_VERSION}.linux-amd64.tar.gz"

    url = f"https://go.dev/dl/{archive_name}"

    import httpx
    with tempfile.NamedTemporaryFile(delete=False, suffix=archive_name) as tmp:
        tmp_path = tmp.name
        print(f"  Fetching {url}")
        with httpx.stream("GET", url, follow_redirects=True, timeout=120) as resp:
            resp.raise_for_status()
            for chunk in resp.iter_bytes(chunk_size=1024 * 64):
                tmp.write(chunk)

    print(f"  Extracting to {TOOLS_DIR}")
    if archive_name.endswith(".zip"):
        with zipfile.ZipFile(tmp_path) as zf:
            zf.extractall(TOOLS_DIR)
    else:
        with tarfile.open(tmp_path) as tf:
            tf.extractall(TOOLS_DIR)

    os.unlink(tmp_path)

    if not os.path.isfile(go_binary()):
        print(f"✗ Go binary not found at {go_binary()}")
        sys.exit(1)

    print(f"✓ Go {GO_VERSION} installed to {GO_DIR}")


def run_go(args, env=None):
    cmd = [go_binary()] + args
    run_env = os.environ.copy()
    run_env["GOPATH"] = os.path.join(TOOLS_DIR, "gopath")
    run_env["GOCACHE"] = os.path.join(TOOLS_DIR, "gocache")
    if env:
        run_env.update(env)
    result = subprocess.run(
        cmd,
        cwd=CODE_DIR,
        env=run_env,
        text=True,
        encoding='utf-8',
        capture_output=True,
    )
    if result.stdout:
        print(result.stdout, end='')
    if result.stderr:
        print(result.stderr, end='')
    return result.returncode


def build_platform(goos, goarch, binary_name):
    output = os.path.join(RELEASED_DIR, binary_name)
    print(f"  Building {binary_name} (GOOS={goos} GOARCH={goarch})...")
    env = {"GOOS": goos, "GOARCH": goarch, "CGO_ENABLED": "0"}
    # Use forward slash for go build output path
    rel_output = os.path.relpath(output, CODE_DIR).replace("\\", "/")
    rc = run_go(["build", "-o", rel_output, "./cmd/kitchensync"], env=env)
    if rc != 0:
        print(f"  ✗ Build failed for {binary_name}")
        return False
    print(f"  ✓ {binary_name}")
    return True


def main():
    build_all = "--all" in sys.argv

    print("KitchenSync Build")
    print("=" * 40)

    # Step 1: Provision Go
    download_go()

    # Verify Go
    rc = run_go(["version"])
    if rc != 0:
        print("✗ Go verification failed")
        sys.exit(1)

    # Step 2: Delete and recreate released/
    import shutil
    if os.path.exists(RELEASED_DIR):
        shutil.rmtree(RELEASED_DIR)
    os.makedirs(RELEASED_DIR)

    # Step 3: go mod tidy
    print("Running go mod tidy...")
    rc = run_go(["mod", "tidy"])
    if rc != 0:
        print("✗ go mod tidy failed")
        sys.exit(1)
    print("✓ go mod tidy")

    # Step 4: Build
    cur = current_platform()
    cur_info = PLATFORMS[cur]

    success = True

    if build_all:
        print("Building all platforms...")
        for plat_info in PLATFORMS.values():
            if not build_platform(plat_info["goos"], plat_info["goarch"], plat_info["binary"]):
                success = False
    else:
        print(f"Building for {cur}...")
        if not build_platform(cur_info["goos"], cur_info["goarch"], cur_info["binary"]):
            success = False

    if not success:
        print("\n✗ Build failed")
        sys.exit(1)

    # List artifacts
    print("\nArtifacts in released/:")
    for f in sorted(os.listdir(RELEASED_DIR)):
        fpath = os.path.join(RELEASED_DIR, f)
        size = os.path.getsize(fpath)
        print(f"  {f} ({size:,} bytes)")

    print("\n✓ Build complete")


if __name__ == "__main__":
    main()
