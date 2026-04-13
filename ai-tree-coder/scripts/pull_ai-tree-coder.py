#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
Pull ai-tree-coder/ from GitHub.

Uses temporary .git directory. Repo name = "ai-tree-coder", owner = logged-in gh user.
Overwrites local files with remote versions (fetch + reset --hard).
"""

import os
import shutil
import stat
import subprocess
import sys
import time
from pathlib import Path


def rm_readonly(func, path, _excinfo):
    os.chmod(path, stat.S_IWRITE)
    func(path)


def rm_readonly_onexc(func, path, _exc):
    os.chmod(path, stat.S_IWRITE)
    func(path)


def rmtree_safe(path: Path, retries: int = 5, delay: float = 0.5) -> None:
    """Remove directory tree, handling Windows read-only files."""
    for attempt in range(retries):
        try:
            if sys.version_info >= (3, 12):
                shutil.rmtree(path, onexc=rm_readonly_onexc)
            else:
                shutil.rmtree(path, onerror=rm_readonly)
            return
        except PermissionError:
            if attempt < retries - 1:
                time.sleep(delay)
            else:
                raise


def run(cmd: list, check: bool = True, capture: bool = False, cwd: str = None):
    """Run a command with UTF-8 encoding."""
    return subprocess.run(
        cmd,
        check=check,
        capture_output=capture,
        text=True,
        encoding="utf-8",
        cwd=cwd,
    )


def get_github_user() -> str:
    """Get logged-in GitHub username via gh CLI."""
    result = run(["gh", "api", "user", "--jq", ".login"], check=False, capture=True)
    if result.returncode != 0:
        print("Error: Not logged in to GitHub. Run: gh auth login", file=sys.stderr)
        sys.exit(1)
    return result.stdout.strip()


def repo_exists(user: str, repo: str) -> bool:
    """Check if repo exists on GitHub."""
    result = run(["gh", "repo", "view", f"{user}/{repo}"], check=False, capture=True)
    return result.returncode == 0


def main() -> int:
    ai_tree_coder_dir = Path(__file__).resolve().parent.parent
    repo_name = "ai-tree-coder"

    os.chdir(ai_tree_coder_dir)

    # Get GitHub user
    user = get_github_user()
    print(f"GitHub user: {user}")

    # Check repo exists
    if not repo_exists(user, repo_name):
        print(f"Error: Repo {user}/{repo_name} does not exist on GitHub", file=sys.stderr)
        return 1

    remote_url = f"https://github.com/{user}/{repo_name}.git"
    git_dir = ai_tree_coder_dir / ".git"
    gitignore = ai_tree_coder_dir / ".gitignore"
    disabled_gitignore = ai_tree_coder_dir / "disabled.gitignore"

    # Rename disabled.gitignore to .gitignore if exists
    if disabled_gitignore.exists() and not gitignore.exists():
        disabled_gitignore.rename(gitignore)

    try:
        # Remove any existing .git
        if git_dir.exists():
            rmtree_safe(git_dir)

        # Init git
        print("Initializing temporary git repo...")
        run(["git", "init", "-b", "main"])
        run(["git", "remote", "add", "origin", remote_url])

        # Fetch remote
        print("Fetching from remote...")
        fetch_result = run(["git", "fetch", "origin", "main"], check=False, capture=True)

        if fetch_result.returncode != 0:
            print(f"Error: Failed to fetch from {user}/{repo_name}", file=sys.stderr)
            print(fetch_result.stderr, file=sys.stderr)
            return 1

        # Reset hard to remote and clean untracked files
        print("Resetting to remote state...")
        run(["git", "reset", "--hard", "origin/main"])
        run(["git", "clean", "-fd"])

        print(f"Successfully pulled from https://github.com/{user}/{repo_name}")
        return 0

    finally:
        # Always clean up .git
        if git_dir.exists():
            rmtree_safe(git_dir)

        # Rename .gitignore back to disabled.gitignore
        if gitignore.exists():
            if disabled_gitignore.exists():
                disabled_gitignore.unlink()
            gitignore.rename(disabled_gitignore)


if __name__ == "__main__":
    sys.exit(main())
