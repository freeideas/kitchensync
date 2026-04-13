#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
Push ai-tree-coder/ to GitHub.

Uses temporary .git directory. Repo name = "ai-tree-coder", owner = logged-in gh user.
Creates repo (private) if it doesn't exist.
"""

import os
import shutil
import stat
import subprocess
import sys
import time
from datetime import datetime
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


def create_repo(user: str, repo: str) -> None:
    """Create private repo on GitHub."""
    print(f"Creating private repo {user}/{repo}...")
    run(["gh", "repo", "create", repo, "--private", "--confirm"], check=True)


def main() -> int:
    ai_tree_coder_dir = Path(__file__).resolve().parent.parent
    repo_name = "ai-tree-coder"

    os.chdir(ai_tree_coder_dir)

    # Get GitHub user
    user = get_github_user()
    print(f"GitHub user: {user}")

    # Create repo if needed
    if not repo_exists(user, repo_name):
        create_repo(user, repo_name)

    remote_url = f"https://github.com/{user}/{repo_name}.git"
    git_dir = ai_tree_coder_dir / ".git"
    gitignore = ai_tree_coder_dir / ".gitignore"
    disabled_gitignore = ai_tree_coder_dir / "disabled.gitignore"

    # Ensure we have a .gitignore
    if disabled_gitignore.exists() and not gitignore.exists():
        disabled_gitignore.rename(gitignore)
    elif not gitignore.exists():
        gitignore.write_text("__pycache__/\n*.pyc\n", encoding="utf-8")

    try:
        # Remove any existing .git
        if git_dir.exists():
            rmtree_safe(git_dir)

        # Init git
        print("Initializing temporary git repo...")
        run(["git", "init", "-b", "main"])
        run(["git", "remote", "add", "origin", remote_url])

        # Configure git user
        run(["git", "config", "user.name", user])
        run(["git", "config", "user.email", f"{user}@users.noreply.github.com"])

        # Fetch remote (may fail if empty repo)
        print("Fetching from remote...")
        fetch_result = run(["git", "fetch", "origin", "main"], check=False, capture=True)

        if fetch_result.returncode == 0:
            # Reset to remote state (soft - keeps working tree)
            run(["git", "reset", "origin/main"])

        # Stage all
        run(["git", "add", "-A"])

        # Check for changes
        result = run(["git", "status", "--porcelain"], capture=True)
        if not result.stdout.strip():
            print("No changes to push")
            return 0

        # Commit
        timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        print(f"Committing: Sync {timestamp}")
        run(["git", "commit", "-m", f"Sync {timestamp}"])

        # Push
        print("Pushing to GitHub...")
        run(["git", "push", "-u", "origin", "main"])

        print(f"Successfully pushed to https://github.com/{user}/{repo_name}")
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
