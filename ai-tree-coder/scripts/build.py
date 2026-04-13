#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""
Build entry point for ai-tree-coder.

Detects the project language by checking which compiler exists in
tools/compiler/, then delegates to the appropriate build_{lang}.py.

Run from a node directory:
    python build.py

The script finds the project root by walking up from cwd until it finds
ai-tree-coder/. It then checks tools/compiler/ for known compiler binaries.

If no compiler is found, it invokes an AI session with DOWNLOAD_COMPILER.md
to provision one.
"""

import sys
import os
import subprocess
import importlib.util
from pathlib import Path

# Fix Windows console encoding for Unicode characters
if sys.stdout.encoding != 'utf-8':
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')

SCRIPT_DIR = Path(__file__).resolve().parent
AI_CODER_DIR = SCRIPT_DIR.parent


def find_project_root() -> Path:
    """Walk up from cwd to find the project root (parent of ai-tree-coder/)."""
    current = Path.cwd().resolve()
    # Also check if we're inside the ai-tree-coder dir itself
    if (current / 'ai-tree-coder').is_dir():
        return current
    for parent in current.parents:
        if (parent / 'ai-tree-coder').is_dir():
            return parent
    # Fallback: ai-tree-coder's own parent
    return AI_CODER_DIR.parent


def detect_language(project_root: Path) -> str | None:
    """
    Detect project language by checking tools/compiler/ for known binaries.

    Returns language name or None if no compiler found.
    """
    compiler_dir = project_root / 'tools' / 'compiler'
    if not compiler_dir.is_dir():
        return None

    # Check for known compiler binaries (.exe works on all platforms)
    checks = {
        'rust': ['cargo/bin/rustc.exe'],
    }

    for lang, paths in checks.items():
        for rel_path in paths:
            if (compiler_dir / rel_path).is_file():
                return lang

    return None


def download_compiler(project_root: Path):
    """
    Invoke AI session with DOWNLOAD_COMPILER.md to provision a compiler.
    """
    prompt_path = AI_CODER_DIR / 'prompts' / 'DOWNLOAD_COMPILER.md'
    if not prompt_path.is_file():
        print(f"✗ No DOWNLOAD_COMPILER.md prompt found at {prompt_path}", file=sys.stderr)
        sys.exit(1)

    print("No compiler found in tools/compiler/. Downloading...", file=sys.stderr, flush=True)

    # Import prompt-ai module
    prompt_ai_path = SCRIPT_DIR / 'prompt-ai.py'
    spec = importlib.util.spec_from_file_location('prompt_ai', prompt_ai_path)
    prompt_ai = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(prompt_ai)

    prompt_text = prompt_path.read_text(encoding='utf-8')
    prompt_ai.get_ai_response_text(
        prompt_text,
        report_type="DOWNLOAD_COMPILER",
        cwd=str(project_root)
    )


def run_build(lang: str, project_root: Path, node_dir: Path):
    """Delegate to the language-specific build script."""
    build_script = SCRIPT_DIR / f'build_{lang}.py'
    if not build_script.is_file():
        print(f"✗ No build script for language '{lang}': {build_script}", file=sys.stderr)
        sys.exit(1)

    # Find uv binary
    import platform
    system = platform.system().lower()
    if system == 'windows':
        uv = AI_CODER_DIR / 'bin' / 'uv.exe'
    elif system == 'darwin':
        uv = AI_CODER_DIR / 'bin' / 'uv.mac'
    else:
        uv = AI_CODER_DIR / 'bin' / 'uv.linux'

    cmd = [str(uv), 'run', '--script', str(build_script)]
    env = os.environ.copy()
    env['PROJECT_ROOT'] = str(project_root)
    env['NODE_DIR'] = str(node_dir)

    result = subprocess.run(
        cmd,
        cwd=str(node_dir),
        env=env,
        text=True,
        encoding='utf-8'
    )

    sys.exit(result.returncode)


def main():
    node_dir = Path.cwd().resolve()
    project_root = find_project_root()

    # Detect language
    lang = detect_language(project_root)

    # If no compiler, try to download one
    if lang is None:
        download_compiler(project_root)
        lang = detect_language(project_root)
        if lang is None:
            print("✗ Compiler download did not produce a recognized compiler in tools/compiler/", file=sys.stderr)
            sys.exit(1)

    print(f"Language: {lang}", file=sys.stderr, flush=True)
    run_build(lang, project_root, node_dir)


if __name__ == '__main__':
    main()
