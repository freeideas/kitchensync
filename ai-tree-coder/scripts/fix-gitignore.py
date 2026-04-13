#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
Fix .gitignore: ensures correct entries are present/absent.
Safe to run multiple times -- won't duplicate or corrupt existing entries.
"""

import os
from pathlib import Path

# Patterns that MUST be ignored
MUST_IGNORE = [
    "ai-tree-coder/",
    "ai-coder/",
    "tmp/",
    "reports/",
    "tools/",
    "CLAUDE.md",
    "CODEX.md",
    "AGENTS.md",
    # Python
    "__pycache__/",
    "*.pyc",
    "*.pyo",
    ".venv/",
    "venv/",
    # Node
    "node_modules/",
    # Credentials
    "creds/",
]

# Patterns that must NOT be ignored (remove if present)
MUST_NOT_IGNORE = [
    "released/",
    "released",
]


def fix_gitignore(repo_root: Path) -> None:
    gitignore_path = repo_root / ".gitignore"

    # Read existing entries
    if gitignore_path.exists():
        lines = gitignore_path.read_text(encoding="utf-8").splitlines()
    else:
        lines = []

    # Remove entries that shouldn't be ignored
    lines = [line for line in lines if line.strip() not in MUST_NOT_IGNORE]

    # Collect existing non-empty, non-comment entries for dedup check
    existing = {line.strip() for line in lines if line.strip() and not line.strip().startswith("#")}

    # Add missing entries
    added = []
    for pattern in MUST_IGNORE:
        if pattern not in existing:
            added.append(pattern)

    if added:
        # Add blank line separator if file has content and doesn't end with blank line
        if lines and lines[-1].strip():
            lines.append("")
        lines.extend(added)

    # Write back
    content = "\n".join(lines)
    if not content.endswith("\n"):
        content += "\n"
    gitignore_path.write_text(content, encoding="utf-8")

    # Report
    if added:
        print(f"Added to .gitignore: {', '.join(added)}")
    else:
        print(".gitignore already up to date")


if __name__ == "__main__":
    repo_root = Path(__file__).resolve().parent.parent.parent
    fix_gitignore(repo_root)
