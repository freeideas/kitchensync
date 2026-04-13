# Run via: ./bin/uv.exe run --script this_file.py
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""
Cleanup script - removes temporary and report directories.

Deletes:
  - ./tmp/
  - ./reports/

This ensures a clean state before running tree-build.
"""

import sys
# Fix Windows console encoding for Unicode characters
if sys.stdout.encoding != 'utf-8':
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')

import os
import shutil
from pathlib import Path


def main():
    """Remove temporary and report directories."""
    # Change to project root (two levels up from this script)
    script_dir = Path(__file__).parent
    project_root = script_dir.parent.parent
    os.chdir(project_root)

    print("Cleaning up temporary directories...")

    dirs_to_clean = ['./tmp', './reports']

    for dir_path in dirs_to_clean:
        path = Path(dir_path)
        if path.exists():
            try:
                shutil.rmtree(path)
                print(f"  OK Removed {dir_path}")
            except Exception as e:
                print(f"  Warning: Could not remove {dir_path}: {e}", file=sys.stderr)
        else:
            print(f"  - {dir_path} (already clean)")

    print()
    return 0


if __name__ == '__main__':
    sys.exit(main())
