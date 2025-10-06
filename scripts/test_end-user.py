#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.8"
# dependencies = []
# ///

import subprocess
import sys
import os
import shutil
import tempfile
import time
from pathlib import Path

# Fix Windows console encoding for Unicode characters
if sys.platform == 'win32':
    import io
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8', errors='replace')
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding='utf-8', errors='replace')


def run_cmd(cmd, check=True, cwd=None):
    """Run command and return result"""
    print(f"Running: {' '.join(cmd)}")
    result = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd)
    if check and result.returncode != 0:
        print(f"FAILED: {result.stderr}")
        return False
    return result


def test_preview_mode(jar_path, test_dir):
    """Test that preview mode doesn't make changes"""
    print("\n=== Testing preview mode (default behavior) ===")

    src = test_dir / "preview_src"
    dst = test_dir / "preview_dst"
    src.mkdir()
    dst.mkdir()

    (src / "file1.txt").write_text("content1")
    (dst / "oldfile.txt").write_text("will not be deleted in preview")

    result = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst)])
    if not result:
        return False

    # In preview mode, destination should be unchanged
    if not (dst / "oldfile.txt").exists():
        print("ERROR: Preview mode deleted file!")
        return False
    if (dst / "file1.txt").exists():
        print("ERROR: Preview mode copied file!")
        return False

    print("✓ Preview mode works correctly")
    return True


def test_actual_sync(jar_path, test_dir):
    """Test actual synchronization with -p=N"""
    print("\n=== Testing actual sync (-p=N) ===")

    src = test_dir / "sync_src"
    dst = test_dir / "sync_dst"
    src.mkdir()
    dst.mkdir()

    (src / "file1.txt").write_text("content1")
    (src / "file2.txt").write_text("content2")
    subdir = src / "subdir"
    subdir.mkdir()
    (subdir / "file3.txt").write_text("content3")

    result = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst), "-p=N", "-v=1"])
    if not result:
        return False

    # Check files were copied
    if not (dst / "file1.txt").exists() or (dst / "file1.txt").read_text() != "content1":
        print("ERROR: file1.txt not synced correctly")
        return False
    if not (dst / "file2.txt").exists() or (dst / "file2.txt").read_text() != "content2":
        print("ERROR: file2.txt not synced correctly")
        return False
    if not (dst / "subdir" / "file3.txt").exists() or (dst / "subdir" / "file3.txt").read_text() != "content3":
        print("ERROR: subdir/file3.txt not synced correctly")
        return False

    print("✓ Actual sync works correctly")
    return True


def test_idempotent_sync(jar_path, test_dir):
    """Test that syncing twice in a row copies nothing the second time"""
    print("\n=== Testing idempotent sync (sync twice → 0 copies second time) ===")

    src = test_dir / "idempotent_src"
    dst = test_dir / "idempotent_dst"
    src.mkdir()
    dst.mkdir()

    (src / "file1.txt").write_text("content1")
    (src / "file2.txt").write_text("content2")
    subdir = src / "subdir"
    subdir.mkdir()
    (subdir / "file3.txt").write_text("content3")

    # First sync
    result = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst), "-p=N", "-v=1"])
    if not result:
        return False

    if "copying file1.txt" not in result.stdout.lower():
        print("ERROR: First sync should copy files")
        return False

    # Second sync - should copy nothing
    result2 = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst), "-p=N", "-v=1"])
    if not result2:
        return False

    if "copying" in result2.stdout.lower():
        print("ERROR: Second sync should not copy anything (modification times should match)")
        print(f"Output: {result2.stdout}")
        return False

    # Verify summary shows 0 files copied
    if "Files copied:     0" not in result2.stdout:
        print("ERROR: Second sync summary should show 0 files copied")
        print(f"Output: {result2.stdout}")
        return False

    print("✓ Idempotent sync works correctly")
    return True


def test_file_update(jar_path, test_dir):
    """Test updating an existing file"""
    print("\n=== Testing file update ===")

    src = test_dir / "update_src"
    dst = test_dir / "update_dst"
    src.mkdir()
    dst.mkdir()

    (src / "file.txt").write_text("original")
    (dst / "file.txt").write_text("original")

    # First sync
    run_cmd(["java", "-jar", str(jar_path), str(src), str(dst), "-p=N", "-v=0"])

    # Update source
    (src / "file.txt").write_text("updated content")

    # Sync again
    result = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst), "-p=N", "-v=1"])
    if not result:
        return False

    if (dst / "file.txt").read_text() != "updated content":
        print("ERROR: File not updated")
        return False

    # Check archive was created
    archive_dir = dst / ".kitchensync"
    if not archive_dir.exists():
        print("ERROR: Archive directory not created")
        return False

    print("✓ File update and archiving works correctly")
    return True


def test_exclusion_patterns(jar_path, test_dir):
    """Test -x exclusion patterns"""
    print("\n=== Testing exclusion patterns (-x) ===")

    src = test_dir / "exclude_src"
    dst = test_dir / "exclude_dst"
    src.mkdir()
    dst.mkdir()

    (src / "keep.txt").write_text("keep this")
    (src / "exclude.tmp").write_text("exclude this")
    (src / ".hidden").write_text("exclude hidden")

    # Use test_dir as cwd to avoid MINGW glob expansion of .*
    result = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst),
                      "-p=N", "-v=0", "-x", "*.tmp", "-x", ".*"], cwd=str(test_dir))
    if not result:
        return False

    if not (dst / "keep.txt").exists():
        print("ERROR: keep.txt was not copied")
        return False
    if (dst / "exclude.tmp").exists():
        print("ERROR: exclude.tmp was copied (should be excluded)")
        return False
    if (dst / ".hidden").exists():
        print("ERROR: .hidden was copied (should be excluded)")
        return False

    print("✓ Exclusion patterns work correctly")
    return True


def test_greater_size_only(jar_path, test_dir):
    """Test -g=Y (greater size only mode)"""
    print("\n=== Testing greater size only mode (-g=Y) ===")

    src = test_dir / "greater_src"
    dst = test_dir / "greater_dst"
    src.mkdir()
    dst.mkdir()

    (src / "larger.txt").write_text("larger content here")
    (src / "smaller.txt").write_text("small")
    (dst / "larger.txt").write_text("small")
    (dst / "smaller.txt").write_text("larger content here")

    result = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst),
                      "-p=N", "-v=0", "-g=Y"])
    if not result:
        return False

    # larger.txt should be copied (source is larger)
    if (dst / "larger.txt").read_text() != "larger content here":
        print("ERROR: larger.txt was not updated")
        return False

    # smaller.txt should NOT be copied (source is smaller)
    if (dst / "smaller.txt").read_text() != "larger content here":
        print("ERROR: smaller.txt was incorrectly updated")
        return False

    print("✓ Greater size only mode works correctly")
    return True


def test_timestamp_filtering(jar_path, test_dir):
    """Test -t=N (skip timestamp-like filenames)"""
    print("\n=== Testing timestamp filtering (default -t=N) ===")

    src = test_dir / "timestamp_src"
    dst = test_dir / "timestamp_dst"
    src.mkdir()
    dst.mkdir()

    (src / "normal.txt").write_text("normal file")
    (src / "backup_20240115_1430.zip").write_text("timestamp file")

    result = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst), "-p=N", "-v=0"])
    if not result:
        return False

    if not (dst / "normal.txt").exists():
        print("ERROR: normal.txt was not copied")
        return False
    if (dst / "backup_20240115_1430.zip").exists():
        print("ERROR: timestamp file was copied (should be filtered)")
        return False

    # Test with -t=Y (include timestamps)
    dst2 = test_dir / "timestamp_dst2"
    dst2.mkdir()
    result = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst2),
                      "-p=N", "-v=0", "-t=Y"])
    if not result:
        return False

    if not (dst2 / "backup_20240115_1430.zip").exists():
        print("ERROR: timestamp file was not copied with -t=Y")
        return False

    print("✓ Timestamp filtering works correctly")
    return True


def test_verbosity_levels(jar_path, test_dir):
    """Test -v=0/1/2 verbosity levels"""
    print("\n=== Testing verbosity levels ===")

    src = test_dir / "verbose_src"
    dst = test_dir / "verbose_dst"
    src.mkdir()
    dst.mkdir()

    (src / "file.txt").write_text("content")

    # Test silent mode
    result = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst), "-p=N", "-v=0"])
    if not result:
        return False
    if "copying" in result.stdout.lower():
        print("ERROR: -v=0 should be silent")
        return False

    # Test normal mode
    dst2 = test_dir / "verbose_dst2"
    dst2.mkdir()
    (src / "file2.txt").write_text("content2")
    result = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst2), "-p=N", "-v=1"])
    if not result:
        return False
    if "copying" not in result.stdout.lower():
        print("ERROR: -v=1 should show operations")
        return False

    print("✓ Verbosity levels work correctly")
    return True


def test_destination_only_cleanup(jar_path, test_dir):
    """Test that destination-only files/directories are archived and removed"""
    print("\n=== Testing destination-only cleanup ===")

    src = test_dir / "cleanup_src"
    dst = test_dir / "cleanup_dst"
    src.mkdir()
    dst.mkdir()

    (src / "keep.txt").write_text("keep this")
    (dst / "keep.txt").write_text("keep this")
    (dst / "remove_me.txt").write_text("should be archived")
    dst_only_dir = dst / "extra_dir"
    dst_only_dir.mkdir()
    (dst_only_dir / "file.txt").write_text("in extra dir")

    result = run_cmd(["java", "-jar", str(jar_path), str(src), str(dst), "-p=N", "-v=1"])
    if not result:
        return False

    # Destination-only items should be gone
    if (dst / "remove_me.txt").exists():
        print("ERROR: Destination-only file was not removed")
        return False
    if (dst / "extra_dir").exists():
        print("ERROR: Destination-only directory was not removed")
        return False

    # They should be in archive
    archive_dir = dst / ".kitchensync"
    if not archive_dir.exists():
        print("ERROR: Archive directory not created")
        return False

    # Find the timestamped archive subdirectory
    archive_subdirs = list(archive_dir.iterdir())
    if not archive_subdirs:
        print("ERROR: No timestamped archive created")
        return False

    archived = archive_subdirs[0]
    if not (archived / "remove_me.txt").exists():
        print("ERROR: File not archived before removal")
        return False
    if not (archived / "extra_dir" / "file.txt").exists():
        print("ERROR: Directory not archived before removal")
        return False

    print("✓ Destination-only cleanup works correctly")
    return True


def test_help(jar_path):
    """Test help output"""
    print("\n=== Testing help output ===")

    result = run_cmd(["java", "-jar", str(jar_path), "--help"], check=False)
    if not result or result.returncode != 0:
        print("ERROR: --help failed")
        return False

    if "Usage:" not in result.stdout:
        print("ERROR: Help output doesn't contain 'Usage:'")
        return False

    print("✓ Help output works correctly")
    return True


def main():
    """Run all end-user tests"""

    project_root = Path(__file__).parent.parent
    os.chdir(project_root)

    # Build the JAR first
    print("Building KitchenSync...")
    build_script = project_root / "scripts" / "build.py"
    result = subprocess.run(["uv", "run", "--script", str(build_script)])
    if result.returncode != 0:
        print("\nBuild failed!")
        sys.exit(1)

    jar_path = project_root / "release" / "kitchensync.jar"
    if not jar_path.exists():
        print(f"\nERROR: {jar_path} not found after build")
        sys.exit(1)

    # Create temporary test directory
    with tempfile.TemporaryDirectory(prefix="kitchensync_test_") as tmpdir:
        test_dir = Path(tmpdir)
        print(f"\nUsing test directory: {test_dir}")

        tests = [
            ("Help output", lambda: test_help(jar_path)),
            ("Preview mode", lambda: test_preview_mode(jar_path, test_dir)),
            ("Actual sync", lambda: test_actual_sync(jar_path, test_dir)),
            ("Idempotent sync", lambda: test_idempotent_sync(jar_path, test_dir)),
            ("File update", lambda: test_file_update(jar_path, test_dir)),
            ("Exclusion patterns", lambda: test_exclusion_patterns(jar_path, test_dir)),
            ("Greater size only", lambda: test_greater_size_only(jar_path, test_dir)),
            ("Timestamp filtering", lambda: test_timestamp_filtering(jar_path, test_dir)),
            ("Verbosity levels", lambda: test_verbosity_levels(jar_path, test_dir)),
            ("Destination-only cleanup", lambda: test_destination_only_cleanup(jar_path, test_dir)),
        ]

        passed = 0
        failed = 0

        for name, test_func in tests:
            try:
                if test_func():
                    passed += 1
                else:
                    failed += 1
                    print(f"✗ {name} FAILED")
            except Exception as e:
                failed += 1
                print(f"✗ {name} FAILED with exception: {e}")
                import traceback
                traceback.print_exc()

        print(f"\n{'='*60}")
        print(f"Test Results: {passed} passed, {failed} failed")
        print(f"{'='*60}")

        if failed > 0:
            sys.exit(1)
        else:
            print("\n🎉 All end-user tests passed!")
            sys.exit(0)


if __name__ == "__main__":
    main()
