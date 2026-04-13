# Run via: ./bin/uv.exe run --script this_file.py
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""
Nuke script: Moves all unprotected items to a timestamped temp directory.

SAFE APPROACH: First runs cleanup.py, then moves everything except protected items
into a timestamped directory in the OS temp folder.

Protected items (NEVER moved):
  - ./README.md
  - ./specs/
  - ./ai-tree-coder/
  - ./ai-coder/
  - ./docs/
  - ./extart/
  - ./creds/

Everything else gets moved to: {TEMP}/nuke_backup_{timestamp}/
"""

import sys
import os
import shutil
import tempfile
import argparse
from pathlib import Path
from datetime import datetime

# Fix Windows console encoding for Unicode characters
if sys.stdout.encoding != 'utf-8':
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')

# Import cleanup module
from cleanup import main as cleanup_main

# Items to PROTECT (never move these)
PROTECTED_ITEMS = {
    'README.md',
    'specs',
    'ai-tree-coder',
    'ai-coder',
    'docs',
    'extart',
    'creds',
}

def get_project_root():
    """Get the project root directory (parent of ai-tree-coder)."""
    script_path = Path(__file__).resolve()
    return script_path.parent.parent.parent

def run_cleanup():
    """Run cleanup.py script first."""
    print("CLEANUP Running cleanup first...")
    try:
        cleanup_main()
    except Exception as e:
        print(f"WARNING  Warning: cleanup failed: {e}")
        print()

def nuke_project(skip_confirm=False):
    """Move all unprotected items to a timestamped temp directory."""
    project_root = get_project_root()

    # Run cleanup first
    run_cleanup()

    print(f"NUKE Nuking project at: {project_root}")
    print()
    print("SHIELD  Protected items (will NOT be moved):")
    for item in sorted(PROTECTED_ITEMS):
        print(f"   - ./{item}")
    print()

    # Find items to move
    items_to_move = []
    for item in project_root.iterdir():
        if item.name not in PROTECTED_ITEMS and not item.name.startswith('.'):
            items_to_move.append(item)

    if not items_to_move:
        print("OK Nothing to move (all items are protected)")
        return

    print("BOX Items to move:")
    for item in sorted(items_to_move):
        item_type = "DIR" if item.is_dir() else "FILE"
        print(f"   {item_type} ./{item.name}")
    print()

    # Confirm operation (skip if -y flag)
    if not skip_confirm:
        response = input("WARNING  Proceed with moving items to temp directory? [y/N]: ")
        if response.lower() != 'y':
            print("FAIL Aborted")
            return

    # Create timestamped backup directory
    timestamp = datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
    temp_dir = Path(tempfile.gettempdir())
    backup_dir = temp_dir / f"nuke_backup_{timestamp}"
    backup_dir.mkdir(parents=True, exist_ok=True)

    print()
    print(f"DIR Moving items to: {backup_dir}")
    print()

    # Move items
    moved_items = []
    for item in items_to_move:
        try:
            dest = backup_dir / item.name
            shutil.move(str(item), str(dest))
            item_type = "DIR" if dest.is_dir() else "FILE"
            print(f"OK Moved {item_type} {item.name}")
            moved_items.append(item.name)
        except Exception as e:
            print(f"X Failed to move {item.name}: {e}")

    print()
    print("OK Nuke complete")
    print()
    print(f"BOX Moved {len(moved_items)} item(s) to:")
    print(f"   {backup_dir}")
    print()
    print("NOTE: Protected items remain in the project directory.")

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description="Nuke: move unprotected items to temp directory")
    parser.add_argument('-y', '--yes', action='store_true', help="Skip confirmation prompt")
    args = parser.parse_args()

    try:
        nuke_project(skip_confirm=args.yes)
    except KeyboardInterrupt:
        print()
        print("FAIL Aborted by user")
        sys.exit(1)
    except Exception as e:
        print(f"FAIL Error: {e}", file=sys.stderr)
        sys.exit(1)
