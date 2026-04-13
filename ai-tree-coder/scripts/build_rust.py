#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""
Rust build script for ai-tree-coder.

Compiles a node's code/ into released/ artifacts:
  - lib.a     (static library, via staticlib crate type)
  - lib.h     (C header, via cbindgen)
  - mcp.exe   (MCP stdio server binary)

Expects environment variables:
  PROJECT_ROOT  -- path to the project root (parent of ai-tree-coder/)
  NODE_DIR      -- path to the current node being built

The AI writes all Rust source files into code/, including:
  - lib.rs     -- library code (compiled to lib.a)
  - mcp.rs     -- MCP stdio server (compiled to mcp.exe)
  - Cargo.toml -- crate manifest (AI writes this with correct dependencies)

Child artifacts (subpjx/*/released/lib.a) are linked automatically.
The AI's Cargo.toml should NOT reference children -- build_rust.py
handles linking via a generated build.rs.
"""

import sys
import os
import shutil
import subprocess
import platform
from pathlib import Path

# Fix Windows console encoding for Unicode characters
if sys.stdout.encoding != 'utf-8':
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')


def find_cargo(project_root: Path) -> Path:
    """Find cargo binary in tools/compiler/."""
    # .exe works on all platforms (Linux/macOS ignore the extension)
    p = project_root / 'tools' / 'compiler' / 'cargo' / 'bin' / 'cargo.exe'
    if p.is_file():
        return p
    print("✗ cargo.exe not found in tools/compiler/cargo/bin/", file=sys.stderr)
    sys.exit(1)


def find_cbindgen(project_root: Path) -> Path | None:
    """Find cbindgen binary (optional -- may not exist yet)."""
    p = project_root / 'tools' / 'compiler' / 'cargo' / 'bin' / 'cbindgen.exe'
    if p.is_file():
        return p
    return None


def collect_child_libs(node_dir: Path) -> list[tuple[str, Path]]:
    """
    Collect child static libraries from subpjx/*/released/lib.a.

    Returns list of (child_name, path_to_lib.a).
    """
    subpjx = node_dir / 'subpjx'
    if not subpjx.is_dir():
        return []

    libs = []
    for child in sorted(subpjx.iterdir()):
        if not child.is_dir():
            continue
        lib_a = child / 'released' / 'lib.a'
        if lib_a.is_file():
            libs.append((child.name, lib_a))
    return libs


def generate_build_rs(node_dir: Path, child_libs: list[tuple[str, Path]]) -> str:
    """
    Generate a build.rs that links child static libraries.

    Returns the build.rs content as a string.
    """
    lines = [
        'fn main() {',
    ]

    for child_name, lib_path in child_libs:
        # Use the directory containing lib.a as the search path
        search_dir = lib_path.parent.resolve()
        # Normalize path separators for Rust (forward slashes)
        search_dir_str = str(search_dir).replace('\\', '/')
        # The library name is "lib" (from lib.a -> -llib)
        # But we need unique names, so we rename during copy
        lines.append(f'    println!("cargo:rustc-link-search=native={search_dir_str}");')
        # The file is lib.a, so the link name is "lib" -- but that collides
        # across children. Instead, we use the full path via link-arg.
        lib_path_str = str(lib_path.resolve()).replace('\\', '/')
        lines.append(f'    println!("cargo:rustc-link-arg={lib_path_str}");')

    lines.append('}')
    return '\n'.join(lines)


def build_node(node_dir: Path, project_root: Path):
    """Build the current node's code/ into released/."""
    code_dir = node_dir / 'code'
    released_dir = node_dir / 'released'

    if not code_dir.is_dir():
        print("✗ No code/ directory in this node", file=sys.stderr)
        sys.exit(1)

    cargo_toml = code_dir / 'Cargo.toml'
    if not cargo_toml.is_file():
        print("✗ No Cargo.toml in code/", file=sys.stderr)
        sys.exit(1)

    cargo = find_cargo(project_root)
    cbindgen = find_cbindgen(project_root)
    child_libs = collect_child_libs(node_dir)

    # Generate build.rs if there are child libraries to link
    if child_libs:
        build_rs_content = generate_build_rs(node_dir, child_libs)
        build_rs_path = code_dir / 'build.rs'
        build_rs_path.write_text(build_rs_content, encoding='utf-8')
        print(f"  Generated build.rs (linking {len(child_libs)} child libs)", file=sys.stderr, flush=True)

    # Set up environment for cargo
    env = os.environ.copy()
    # Point CARGO_HOME to avoid polluting system
    cargo_home = project_root / 'tools' / 'compiler' / 'cargo'
    env['CARGO_HOME'] = str(cargo_home)
    # Add cargo bin to PATH
    cargo_bin = cargo_home / 'bin'
    env['PATH'] = str(cargo_bin) + os.pathsep + env.get('PATH', '')

    # Windows: ensure MSVC environment is available
    if platform.system() == 'Windows':
        _setup_msvc_env(env)

    # Build the library (staticlib)
    print(f"  Building library...", file=sys.stderr, flush=True)
    result = subprocess.run(
        [str(cargo), 'build', '--release', '--manifest-path', str(cargo_toml)],
        cwd=str(code_dir),
        env=env,
        text=True,
        encoding='utf-8',
        capture_output=True
    )
    if result.returncode != 0:
        print(f"✗ cargo build (library) failed:\n{result.stderr}", file=sys.stderr)
        sys.exit(result.returncode)

    print(f"  Library build OK", file=sys.stderr, flush=True)

    # Find the built artifacts
    target_release = code_dir / 'target' / 'release'

    # Create released/ directory
    if released_dir.exists():
        shutil.rmtree(released_dir)
    released_dir.mkdir(parents=True)

    # Copy static library
    # Rust names it lib{crate_name}.a (Unix) or {crate_name}.lib (Windows MSVC)
    lib_found = False
    for pattern in ['*.a', '*.lib']:
        for f in target_release.glob(pattern):
            if f.name.startswith('lib') or f.suffix == '.lib':
                # Skip deps/ artifacts
                if 'deps' in str(f.relative_to(target_release)):
                    continue
                dest = released_dir / 'lib.a' if f.suffix == '.a' else released_dir / 'lib.a'
                shutil.copy2(f, dest)
                lib_found = True
                print(f"  Copied {f.name} -> released/lib.a", file=sys.stderr, flush=True)
                break
        if lib_found:
            break

    if not lib_found:
        print("✗ No static library artifact found in target/release/", file=sys.stderr)
        sys.exit(1)

    # Copy MCP binary (if it exists)
    # Cargo produces mcp.exe on Windows, mcp on Unix -- check both
    for name in ['mcp.exe', 'mcp']:
        mcp_bin = target_release / name
        if mcp_bin.is_file():
            shutil.copy2(mcp_bin, released_dir / 'mcp.exe')
            print(f"  Copied {name} -> released/mcp.exe", file=sys.stderr, flush=True)
            break

    # Generate C header via cbindgen (if available)
    if cbindgen:
        lib_rs = code_dir / 'src' / 'lib.rs'
        if not lib_rs.is_file():
            lib_rs = code_dir / 'lib.rs'
        if lib_rs.is_file():
            header_path = released_dir / 'lib.h'
            result = subprocess.run(
                [str(cbindgen), '--crate', _get_crate_name(cargo_toml),
                 '--output', str(header_path), str(code_dir)],
                env=env,
                text=True,
                encoding='utf-8',
                capture_output=True
            )
            if result.returncode == 0:
                print(f"  Generated released/lib.h", file=sys.stderr, flush=True)
            else:
                # cbindgen failure is not fatal -- node may not export C API
                print(f"  cbindgen skipped (no C exports or error)", file=sys.stderr, flush=True)

    print(f"✓ Build complete: {released_dir}", file=sys.stderr, flush=True)


def _get_crate_name(cargo_toml: Path) -> str:
    """Extract crate name from Cargo.toml (simple parser)."""
    for line in cargo_toml.read_text(encoding='utf-8').splitlines():
        line = line.strip()
        if line.startswith('name'):
            # name = "foo"
            parts = line.split('=', 1)
            if len(parts) == 2:
                return parts[1].strip().strip('"').strip("'")
    return 'node'


def _setup_msvc_env(env: dict):
    """
    Try to set up MSVC environment on Windows.

    Looks for vcvarsall.bat and runs it to get the environment variables.
    If not found, assumes the environment is already set up (e.g., from
    a Developer Command Prompt).
    """
    # Common vcvarsall.bat locations
    program_files = os.environ.get('ProgramFiles(x86)', r'C:\Program Files (x86)')
    vs_paths = [
        Path(program_files) / 'Microsoft Visual Studio' / '2022' / 'Community' / 'VC' / 'Auxiliary' / 'Build' / 'vcvarsall.bat',
        Path(program_files) / 'Microsoft Visual Studio' / '2022' / 'Professional' / 'VC' / 'Auxiliary' / 'Build' / 'vcvarsall.bat',
        Path(program_files) / 'Microsoft Visual Studio' / '2022' / 'BuildTools' / 'VC' / 'Auxiliary' / 'Build' / 'vcvarsall.bat',
        Path(program_files) / 'Microsoft Visual Studio' / '2022' / 'Enterprise' / 'VC' / 'Auxiliary' / 'Build' / 'vcvarsall.bat',
    ]

    vcvarsall = None
    for p in vs_paths:
        if p.is_file():
            vcvarsall = p
            break

    if vcvarsall is None:
        return  # No MSVC found, hope for the best

    try:
        # Run vcvarsall and capture resulting environment
        result = subprocess.run(
            f'cmd.exe /c ""{vcvarsall}" x64 && set"',
            capture_output=True,
            text=True,
            encoding='utf-8',
            shell=True
        )
        if result.returncode == 0:
            for line in result.stdout.splitlines():
                if '=' in line:
                    key, _, value = line.partition('=')
                    env[key] = value
    except Exception:
        pass  # Non-fatal


def main():
    project_root = Path(os.environ.get('PROJECT_ROOT', '.')).resolve()
    node_dir = Path(os.environ.get('NODE_DIR', '.')).resolve()

    build_node(node_dir, project_root)


if __name__ == '__main__':
    main()
