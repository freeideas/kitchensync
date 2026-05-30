#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end verification for reqs/015_transport-operations.md."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable


if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path(r"C:/Users/human/Desktop/prjx/kitchensync")
PROJECT_DIR = Path(r"C:/Users/human/Desktop/prjx/kitchensync/proj")
RELEASED_BINARY = Path(
    r"C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.exe"
    if os.name == "nt"
    else r"C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync"
)


def _run_case(name: str, failures: list[str], fn: Callable[[], None]) -> None:
    try:
        fn()
    except AssertionError as exc:
        failures.append(f"{name}: {exc}")
    except Exception as exc:  # noqa: BLE001
        failures.append(f"{name}: unexpected exception: {exc!r}")


def _fail(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def _run_kitchensync(
    args: list[str], *, cwd: Path, timeout_seconds: float = 20.0
) -> subprocess.CompletedProcess[str] | None:
    cmd = [str(RELEASED_BINARY), *args]
    try:
        return subprocess.run(
            cmd,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(
            args=cmd,
            returncode=124,
            stdout="",
            stderr=f"kitchensync timed out after {timeout_seconds:.1f}s",
        )
    except (FileNotFoundError, OSError) as exc:
        return subprocess.CompletedProcess(
            args=cmd,
            returncode=127,
            stdout="",
            stderr=f"failed to launch kitchensync: {exc}",
        )


def _assert_success(
    failures: list[str],
    req_id: str,
    result: subprocess.CompletedProcess[str] | None,
    command: list[str],
) -> bool:
    if result is None:
        failures.append(f"{req_id}: kitchensync invocation failed or timed out for {command!r}")
        return False
    _fail(
        failures,
        result.returncode == 0,
        f"{req_id}: expected exit code 0 for {command!r}, got {result.returncode}. stdout={result.stdout!r} stderr={result.stderr!r}",
    )
    if result.returncode != 0:
        return False
    _fail(failures, not result.stderr.strip(), f"{req_id}: expected empty stderr, got {result.stderr!r}")
    return True


def _write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def _file_url(path: Path) -> str:
    return path.resolve().as_uri()


def _has_legacy_backup(root: Path, filename: str, expected: str) -> bool:
    bak = root / ".kitchensync" / "BAK"
    if not bak.exists():
        return False
    for candidate in bak.rglob(filename):
        if not candidate.is_file():
            continue
        try:
            if candidate.read_text(encoding="utf-8", errors="replace") == expected:
                return True
        except OSError:
            continue
    return False


def check_nested_create_and_stream_write(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_015_nested_write_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        sink = workspace / "sink"

        payload = b"payload-bytes-for-transport-write"
        _write_bytes(canon / "layered" / "tree" / "payload.bin", payload)

        result = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
        if not _assert_success(failures, "015.13/015.14/015.15/015.19", result, [f"+{canon}", str(sink)]):
            return

        target = sink / "layered" / "tree" / "payload.bin"
        _fail(failures, target.is_file(), "015.13/015.14/015.15: target file not created in sink")
        if target.is_file():
            _fail(failures, target.read_bytes() == payload, "015.13/015.15: target bytes changed after stream write completion")
        _fail(
            failures,
            (sink / "layered" / "tree").is_dir(),
            "015.14/015.19: missing parent directories for nested destination write",
        )


def check_replace_existing_file_and_archive_previous(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_015_replace_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        sink = workspace / "sink"

        _write_text(canon / "replace.txt", "winning")
        _write_text(sink / "replace.txt", "old")

        result = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
        if not _assert_success(failures, "015.16/015.17", result, [f"+{canon}", str(sink)]):
            return

        target = sink / "replace.txt"
        _fail(failures, target.is_file(), "015.16: destination file not present after copy")
        if target.is_file():
            _fail(
                failures,
                target.read_text(encoding="utf-8") == "winning",
                "015.16/015.17: destination content was not replaced from source",
            )
        _fail(
            failures,
            _has_legacy_backup(sink, "replace.txt", "old"),
            "015.16/015.17: replacing destination file did not archive prior content into BAK",
        )


def check_file_delete(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_015_delete_file_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        sink = workspace / "sink"

        _write_text(canon / "keep.txt", "stay")
        _write_text(canon / "delete-me.txt", "remove-me")

        first = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
        if not _assert_success(failures, "015.18", first, [f"+{canon}", str(sink)]):
            return

        (canon / "delete-me.txt").unlink()

        second = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
        if not _assert_success(failures, "015.18", second, [f"+{canon}", str(sink)]):
            return

        _fail(failures, (sink / "delete-me.txt").exists() is False, "015.18: source file deletion did not remove destination file")
        _fail(failures, (sink / "keep.txt").exists(), "015.18: unrelated destination file was removed during delete handling")


def check_empty_directory_deletion(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_015_delete_dir_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        sink = workspace / "sink"

        _write_text(canon / "to-remove" / "child.txt", "child")

        first = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
        if not _assert_success(failures, "015.20", first, [f"+{canon}", str(sink)]):
            return

        shutil.rmtree(canon / "to-remove")

        second = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
        if not _assert_success(failures, "015.20", second, [f"+{canon}", str(sink)]):
            return

        _fail(failures, not (sink / "to-remove" / "child.txt").exists(), "015.20: removed source directory's child remained at destination")
        _fail(failures, not (sink / "to-remove").exists(), "015.20: empty destination directory was not deleted")


def check_mod_time_update(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_015_mod_time_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        sink = workspace / "sink"

        source_file = canon / "mod.txt"
        _write_text(source_file, "timestamped")
        requested = datetime(2025, 1, 1, 10, 20, 30, tzinfo=timezone.utc)
        requested_unix = requested.timestamp()
        os.utime(source_file, (requested_unix, requested_unix))

        result = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
        if not _assert_success(failures, "015.21", result, [f"+{canon}", str(sink)]):
            return

        sink_file = sink / "mod.txt"
        _fail(failures, sink_file.is_file(), "015.21: destination file missing after copy")
        if not sink_file.is_file():
            return

        sink_ts = datetime.fromtimestamp(sink_file.stat().st_mtime, tz=timezone.utc)
        drift = abs((sink_ts - requested).total_seconds())
        _fail(
            failures,
            drift <= 2.0,
            f"015.21: destination mtime drift was {drift:.3f}s; expected {requested.isoformat()}, got {sink_ts.isoformat()}",
        )


def check_omit_special_entry_types(failures: list[str]) -> None:
    if os.name == "nt":
        # not reasonably testable: 015.6 -- symbolic links and special files are not uniformly observable on Windows test hosts
        # not reasonably testable: 015.7 -- special-file entries are not uniformly observable on Windows test hosts
        return

    with tempfile.TemporaryDirectory(prefix="ks_015_omit_special_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        sink = workspace / "sink"

        _write_text(canon / "kept.txt", "keep")
        kept_dir = canon / "kept-dir"
        kept_dir.mkdir()
        _write_text(kept_dir / "kept-child.txt", "child")

        symlink_path = canon / "link-to-keep.txt"
        try:
            os.symlink(kept_dir, canon / "link-dir")
            os.symlink(canon / "kept.txt", symlink_path)
        except OSError:
            # not reasonably testable: 015.6 -- symbolic link omission is not supported by runtime privileges on this host
            return

        fifo = canon / "special.fifo"
        try:
            os.mkfifo(fifo)
        except OSError:
            # not reasonably testable: 015.7 -- special file omission for FIFOs unsupported on this host
            fifo = None

        result = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
        if not _assert_success(failures, "015.6/015.7", result, [f"+{canon}", str(sink)]):
            return

        _fail(failures, (sink / "kept.txt").is_file(), "015.6: regular file was not synced")
        _fail(failures, not (sink / "link-to-keep.txt").exists(), "015.6: symbolic link was synced but must be omitted")
        _fail(failures, not (sink / "link-dir").exists(), "015.6: symbolic link directory was synced but must be omitted")
        if fifo is not None:
            _fail(
                failures,
                not (sink / "special.fifo").exists(),
                "015.7: special FIFO entry was synced but must be omitted",
            )


def check_case_preservation(failures: list[str]) -> None:
    if os.name == "nt":
        # not reasonably testable: 015.24 -- case-sensitive filename preservation cannot be distinguished reliably on case-insensitive hosts
        return

    with tempfile.TemporaryDirectory(prefix="ks_015_case_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        sink = workspace / "sink"

        lower = canon / "case_name.txt"
        upper = canon / "Case_Name.txt"
        _write_text(lower, "lower")
        _write_text(upper, "upper")

        if (
            lower.name == upper.name
            or lower.read_text(encoding="utf-8") != "lower"
            or upper.read_text(encoding="utf-8") != "upper"
        ):
            # not reasonably testable: 015.24 -- case-distinct filenames are not represented distinctly here
            return

        result = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
        if not _assert_success(failures, "015.24", result, [f"+{canon}", str(sink)]):
            return

        _fail(failures, (sink / lower.name).is_file(), "015.24: lowercase filename missing on destination")
        _fail(failures, (sink / upper.name).is_file(), "015.24: uppercase filename missing on destination")
        if (sink / lower.name).is_file() and (sink / upper.name).is_file():
            _fail(failures, (sink / lower.name).read_text(encoding="utf-8") == "lower", "015.24: lowercase filename content changed")
            _fail(failures, (sink / upper.name).read_text(encoding="utf-8") == "upper", "015.24: uppercase filename content changed")


def main() -> int:
    failures: list[str] = []

    _fail(failures, RELEASED_BINARY.is_file(), f"precondition: released executable missing at {RELEASED_BINARY}")

    # not reasonably testable: 015.1 -- file:// and sftp:// transport equivalence requires an in-process SFTP fixture
    # not reasonably testable: 015.2 -- immediate-child-only listing semantics are internal to traversal behavior
    # not reasonably testable: 015.3 -- list_dir field-level details (name, is_dir, mod_time, byte_size) are not directly surfaced on CLI
    # not reasonably testable: 015.4 -- list_dir file byte_size value is not directly surfaced on CLI
    # not reasonably testable: 015.5 -- list_dir directory byte_size value is not directly surfaced on CLI
    # not reasonably testable: 015.8 -- transport-level stat output is not directly surfaced on CLI
    # not reasonably testable: 015.9 -- stat not found for missing path is not directly surfaced on CLI
    # not reasonably testable: 015.10 -- stat not found for non-regular entries is not directly surfaced on CLI
    # not reasonably testable: 015.11 -- stream read chunk boundaries are not observable from CLI outputs
    # not reasonably testable: 015.12 -- stream read handle-close semantics are not observable from CLI outputs
    # not reasonably testable: 015.22 -- transport error category mapping needs transport-level fault injection beyond this CLI surface
    # not reasonably testable: 015.23 -- connection-drop/timeout error-category mapping needs in-process SFTP transport and fault injection

    if failures:
        print(f"FAIL: test_015_transport_operations.py (precondition)")
        for failure in failures:
            print(f"  - {failure}")
        return 1

    _run_case("015.13/015.14/015.15/015.19", failures, lambda: check_nested_create_and_stream_write(failures))
    _run_case("015.16/015.17", failures, lambda: check_replace_existing_file_and_archive_previous(failures))
    _run_case("015.18", failures, lambda: check_file_delete(failures))
    _run_case("015.20", failures, lambda: check_empty_directory_deletion(failures))
    _run_case("015.21", failures, lambda: check_mod_time_update(failures))
    _run_case("015.6/015.7", failures, lambda: check_omit_special_entry_types(failures))
    _run_case("015.24", failures, lambda: check_case_preservation(failures))

    if failures:
        print("FAIL: test_015_transport_operations.py")
        for index, failure in enumerate(failures, start=1):
            print(f"  {index:02d}. {failure}")
        return 1

    print("PASS: test_015_transport_operations.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
