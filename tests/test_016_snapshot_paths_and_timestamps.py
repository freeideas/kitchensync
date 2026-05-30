#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

from __future__ import annotations

import os
import re
import sqlite3
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable


if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path(r"C:\Users\human\Desktop\prjx\kitchensync")
PROJECT_DIR = Path(r"C:\Users\human\Desktop\prjx\kitchensync\proj")
WINDOWS_RELEASED_BINARY = Path(r"C:\Users\human\Desktop\prjx\kitchensync\released\kitchensync.exe")
POSIX_RELEASED_BINARY = Path(r"C:\Users\human\Desktop\prjx\kitchensync\released\kitchensync")
RELEASED_BINARY = WINDOWS_RELEASED_BINARY if os.name == "nt" else POSIX_RELEASED_BINARY

TIMESTAMP_FORMAT = "%Y-%m-%d_%H-%M-%S_%fZ"
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")
BASE62_RE = re.compile(r"^[0-9A-Za-z]{11}$")


def _fail(failures: list[str], condition: bool, req: str, message: str) -> None:
    if not condition:
        failures.append(f"{req}: {message}")


def _run_case(name: str, failures: list[str], fn) -> None:
    try:
        fn()
    except AssertionError as exc:
        failures.append(f"{name}: assertion failed: {exc}")
    except Exception as exc:  # noqa: BLE001
        failures.append(f"{name}: unexpected exception: {exc!r}")


def _run_kitchensync(args: list[str], *, cwd: Path, timeout_seconds: float = 30.0) -> subprocess.CompletedProcess[str]:
    command = [str(RELEASED_BINARY), *args]
    try:
        return subprocess.run(
            command,
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
            args=command,
            returncode=124,
            stdout="",
            stderr="kitchensync invocation timed out",
        )
    except (FileNotFoundError, OSError) as exc:
        return subprocess.CompletedProcess(
            args=command,
            returncode=127,
            stdout="",
            stderr=f"failed to launch released executable: {exc}",
        )


def _assert_success(
    failures: list[str],
    req: str,
    result: subprocess.CompletedProcess[str],
    args: list[str],
) -> bool:
    if result is None:
        failures.append(f"{req}: command returned no result for {args!r}")
        return False
    _fail(
        failures,
        result.returncode == 0,
        req,
        f"expected exit code 0 for {args!r}, got {result.returncode}. stdout={result.stdout!r}; stderr={result.stderr!r}",
    )
    if result.returncode == 0:
        return True
    if result.stderr:
        failures.append(f"{req}: expected empty stderr for {args!r}, got {result.stderr!r}")
    return False


def _write_text(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value, encoding="utf-8")


def _parse_timestamp(value: str | None) -> datetime | None:
    if value is None:
        return None
    if TIMESTAMP_RE.fullmatch(value) is None:
        return None
    try:
        return datetime.strptime(value, TIMESTAMP_FORMAT).replace(tzinfo=timezone.utc)
    except ValueError:
        return None


def _load_snapshot_rows(peer_root: Path) -> list[dict[str, object]]:
    db_path = peer_root / ".kitchensync" / "snapshot.db"
    if not db_path.is_file():
        return []

    conn: sqlite3.Connection | None = None
    try:
        conn = sqlite3.connect(str(db_path))
        conn.row_factory = sqlite3.Row
        rows = conn.execute(
            "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time FROM snapshot;"
        ).fetchall()
        return [dict(row) for row in rows]
    except Exception:
        return []
    finally:
        if conn is not None:
            conn.close()


def _snapshot_by_path(peer_root: Path) -> tuple[dict[str, dict[str, object]], str | None]:
    rows = _load_snapshot_rows(peer_root)
    by_id: dict[str, dict[str, object]] = {}
    for row in rows:
        row_id = row.get("id")
        if row_id is None:
            continue
        by_id[str(row_id)] = row

    parent_refs = {
        str(row["parent_id"])
        for row in by_id.values()
        if row.get("parent_id") is not None and str(row["parent_id"]) not in by_id
    }
    if len(parent_refs) == 1:
        sentinel = next(iter(parent_refs))
    else:
        sentinel = next(iter(parent_refs), None)

    memo: dict[str, str | None] = {}
    visiting: set[str] = set()

    def resolve(row_id: str) -> str | None:
        if row_id in memo:
            return memo[row_id]
        if row_id in visiting:
            return None
        row = by_id.get(row_id)
        if row is None:
            return None
        parent_id = row.get("parent_id")
        basename = row.get("basename")
        if parent_id is None or basename is None:
            return None
        visiting.add(row_id)
        parent_text = str(parent_id)
        if parent_text == sentinel:
            path = str(basename)
        elif parent_text in by_id:
            parent_path = resolve(parent_text)
            if parent_path is None:
                path = None
            else:
                path = f"{parent_path}/{basename}"
        else:
            path = None
        visiting.remove(row_id)
        memo[row_id] = path
        return path

    path_to_row: dict[str, dict[str, object]] = {}
    for row_id, row in by_id.items():
        path = resolve(row_id)
        if path is None or not path:
            continue
        path_to_row[path] = row
    return path_to_row, sentinel


def _snapshot_row(peer_root: Path, relative: str) -> dict[str, object] | None:
    rows = _snapshot_by_path(peer_root)[0]
    return rows.get(relative)


def _collect_timestamped_meta_dirs(peer_root: Path, label: str) -> list[tuple[str, datetime]]:
    root = peer_root / ".kitchensync" / label
    if not root.is_dir():
        return []

    result: list[tuple[str, datetime]] = []
    for child in sorted(root.iterdir(), key=lambda p: p.name):
        if not child.is_dir():
            continue
        if TIMESTAMP_RE.fullmatch(child.name) is None:
            continue
        parsed = _parse_timestamp(child.name)
        if parsed is None:
            continue
        result.append((child.name, parsed))
    return result


def _strictly_increasing(values: list[str]) -> bool:
    return all(previous < current for previous, current in zip(values, values[1:]))


def check_snapshot_identifiers_and_parent_links(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_016_ids_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        peer = workspace / "peer"
        canon.mkdir()
        peer.mkdir()

        (canon / "nested").mkdir(parents=True)
        _write_text(canon / "nested" / "file.txt", "nested")
        _write_text(canon / "root.txt", "root")
        _write_text(canon / "top.txt", "top")

        result = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        if not _assert_success(failures, "016.1/016.2/016.3/016.5/016.6/016.7", result, [f"+{canon}", str(peer)]):
            return

        path_rows, sentinel = _snapshot_by_path(peer)
        _fail(failures, "" not in path_rows, "016.7", "snapshot root appears as a row")

        for path, row in path_rows.items():
            row_id = row.get("id")
            parent_id = row.get("parent_id")
            basename = row.get("basename")

            _fail(
                failures,
                BASE62_RE.fullmatch(str(row_id or "")) is not None,
                "016.1",
                f"path {path!r} row id is not 11-char base62: {row_id!r}",
            )
            if parent_id is not None:
                _fail(
                    failures,
                    BASE62_RE.fullmatch(str(parent_id)) is not None,
                    "016.5",
                    f"path {path!r} parent_id is not 11-char base62: {parent_id!r}",
                )
            _fail(
                failures,
                isinstance(basename, str) and basename != "" and "/" not in basename and "\\" not in basename,
                "016.2/016.3",
                f"path {path!r} has invalid basename {basename!r}",
            )
            _fail(
                failures,
                not path.startswith("/"),
                "016.2/016.3",
                f"path {path!r} has leading slash",
            )
            _fail(
                failures,
                not path.endswith("/"),
                "016.2/016.3",
                f"path {path!r} has trailing slash",
            )

        top_level = {path: row for path, row in path_rows.items() if "/" not in path}
        if top_level:
            top_level_parents = {str(row.get("parent_id")) for row in top_level.values() if row.get("parent_id") is not None}
            _fail(failures, bool(top_level_parents), "016.6", "top-level rows did not expose a parent_id sentinel")
            if top_level_parents:
                _fail(
                    failures,
                    len(top_level_parents) == 1,
                    "016.6",
                    f"top-level parent IDs were not uniform: {sorted(top_level_parents)}",
                )
                if len(top_level_parents) == 1:
                    expected_parent = next(iter(top_level_parents))
                    for path, row in top_level.items():
                        _fail(
                            failures,
                            str(row.get("parent_id")) == expected_parent,
                            "016.6",
                            f"top-level path {path!r} had unexpected parent_id {row.get('parent_id')!r}",
                        )
                    if sentinel is not None:
                        _fail(
                            failures,
                            sentinel == expected_parent,
                            "016.6",
                            f"snapshot parent sentinel {sentinel!r} did not match top-level parent {expected_parent!r}",
                        )

        nested = path_rows.get("nested")
        nested_file = path_rows.get("nested/file.txt")
        _fail(
            failures,
            nested is not None and nested_file is not None,
            "016.5",
            "expected nested parent and child rows in snapshot",
        )
        if nested is not None and nested_file is not None:
            _fail(
                failures,
                str(nested_file.get("parent_id")) == str(nested.get("id")),
                "016.5",
                "child.parent_id was not the parent row id",
            )


def check_type_transition_reuses_snapshot_id(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_016_type_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        peer = workspace / "peer"
        canon.mkdir()
        peer.mkdir()

        shared = canon / "shared"
        shared.mkdir()
        _write_text(shared / "inner.txt", "initial-directory")

        first = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        if not _assert_success(failures, "016.4", first, [f"+{canon}", str(peer)]):
            return

        first_row = _snapshot_row(peer, "shared")
        _fail(failures, first_row is not None, "016.4", "missing snapshot row for initial directory path 'shared'")
        if first_row is None:
            return
        first_id = str(first_row.get("id"))

        for child in shared.iterdir():
            if child.is_file():
                child.unlink()
            else:
                child.rmdir()
        shared.rmdir()
        _write_text(shared, "now-file")

        second = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        if not _assert_success(failures, "016.4", second, [f"+{canon}", str(peer)]):
            return

        second_row = _snapshot_row(peer, "shared")
        _fail(failures, second_row is not None, "016.4", "missing snapshot row for 'shared' after changing type to file")
        if second_row is None:
            return
        _fail(
            failures,
            str(second_row.get("id")) == first_id,
            "016.4",
            f"path id for 'shared' changed across type transition: {first_id!r} -> {second_row.get('id')!r}",
        )


def check_snapshot_timestamps_format_and_lexicographic_order(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_016_timestamps_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        peer = workspace / "peer"
        canon.mkdir()
        peer.mkdir()

        _write_text(canon / "keep.txt", "keep")
        _write_text(canon / "vanish.txt", "vanish")

        first = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        if not _assert_success(failures, "016.8/016.9/016.10/016.11/016.12/016.20", first, [f"+{canon}", str(peer)]):
            return

        first_vanish = _snapshot_row(peer, "vanish.txt")
        _fail(failures, first_vanish is not None, "016.20", "baseline vanish.txt row not present before deletion")
        if first_vanish is None:
            return
        first_vanish_seen = str(first_vanish.get("last_seen"))

        _write_text(canon / "keep.txt", "keep-updated")
        (canon / "vanish.txt").unlink()

        second = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        if not _assert_success(failures, "016.8/016.9/016.10/016.11/016.12/016.20", second, [f"+{canon}", str(peer)]):
            return

        path_rows = _snapshot_by_path(peer)[0]
        values: list[str] = []
        parsed_values: list[datetime] = []

        for path, row in path_rows.items():
            for key in ("mod_time", "last_seen", "deleted_time"):
                raw_value = row.get(key)
                if raw_value is None:
                    continue
                value = str(raw_value)
                parsed = _parse_timestamp(value)
                _fail(
                    failures,
                    parsed is not None,
                    "016.8/016.9/016.10",
                    f"{key} for {path!r} is not valid timestamp format: {value!r}",
                )
                if parsed is not None:
                    values.append(value)
                    parsed_values.append(parsed)

        _fail(failures, bool(values), "016.8", "no snapshot timestamp values were observed")
        if values:
            sorted_values = sorted(values)
            sorted_by_time = [value.strftime(TIMESTAMP_FORMAT) for value in sorted(parsed_values)]
            _fail(
                failures,
                sorted_values == sorted_by_time,
                "016.11",
                "snapshot timestamp values are not in lexicographic chronological order",
            )

        vanished = path_rows.get("vanish.txt")
        if vanished is not None:
            current_deleted = str(vanished.get("deleted_time"))
            _fail(
                failures,
                current_deleted == first_vanish_seen,
                "016.20",
                f"vanish.txt deleted_time {current_deleted!r} did not copy prior last_seen {first_vanish_seen!r}",
            )


def check_last_seen_refreshes_on_update(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_016_last_seen_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        peer = workspace / "peer"
        canon.mkdir()
        peer.mkdir()

        _write_text(canon / "refresh.txt", "v1")
        first = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        if not _assert_success(failures, "016.17", first, [f"+{canon}", str(peer)]):
            return

        first_row = _snapshot_row(peer, "refresh.txt")
        _fail(failures, first_row is not None, "016.17", "seed refresh row not present")
        if first_row is None:
            return
        first_seen = _parse_timestamp(str(first_row.get("last_seen")))
        _fail(failures, first_seen is not None, "016.17", "initial last_seen was not parseable")
        if first_seen is None:
            return

        time.sleep(0.05)
        _write_text(canon / "refresh.txt", "v2")
        second = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        if not _assert_success(failures, "016.17", second, [f"+{canon}", str(peer)]):
            return

        second_row = _snapshot_row(peer, "refresh.txt")
        _fail(failures, second_row is not None, "016.17", "refresh row disappeared after rewrite")
        if second_row is None:
            return
        second_seen = _parse_timestamp(str(second_row.get("last_seen")))
        _fail(failures, second_seen is not None, "016.17", "updated last_seen was not parseable")
        if second_seen is None:
            return
        _fail(
            failures,
            second_seen > first_seen,
            "016.17",
            f"expected last_seen to refresh (fresh timestamp), got first={first_seen} and second={second_seen}",
        )


def check_bak_tmp_timestamp_paths_and_freshness(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_016_bak_tmp_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        peer = workspace / "peer"
        canon.mkdir()
        peer.mkdir()

        _write_text(canon / "a.txt", "A-one")
        _write_text(canon / "b.txt", "B-one")
        _write_text(peer / "a.txt", "A-old")
        _write_text(peer / "b.txt", "B-old")

        first = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        if not _assert_success(failures, "016.13/016.14/016.16/016.18/016.19", first, [f"+{canon}", str(peer)]):
            return

        bak_first = _collect_timestamped_meta_dirs(peer, "BAK")
        tmp_first = _collect_timestamped_meta_dirs(peer, "TMP")
        bak_names_first = [name for name, _ in bak_first]
        tmp_names_first = [name for name, _ in tmp_first]
        _fail(failures, bool(bak_first), "016.13", "BAK timestamp segment missing after replacement")
        _fail(failures, bool(tmp_first), "016.14", "TMP timestamp segment missing after replacement")

        _fail(failures, _strictly_increasing(sorted(bak_names_first)), "016.16/016.18", "BAK timestamps were not strictly increasing")
        _fail(failures, _strictly_increasing(sorted(tmp_names_first)), "016.16/016.19", "TMP timestamps were not strictly increasing")

        for name in bak_names_first:
            _fail(failures, TIMESTAMP_RE.fullmatch(name) is not None, "016.13", f"BAK segment {name!r} is not in required timestamp format")
        for name in tmp_names_first:
            _fail(failures, TIMESTAMP_RE.fullmatch(name) is not None, "016.14", f"TMP segment {name!r} is not in required timestamp format")

        _write_text(canon / "a.txt", "A-two")
        _write_text(canon / "b.txt", "B-two")
        _write_text(canon / "c.txt", "C-new")
        _write_text(peer / "c.txt", "C-old")

        second = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        if not _assert_success(failures, "016.18/016.19", second, [f"+{canon}", str(peer)]):
            return

        bak_second = _collect_timestamped_meta_dirs(peer, "BAK")
        tmp_second = _collect_timestamped_meta_dirs(peer, "TMP")
        bak_names_second = [name for name, _ in bak_second]
        tmp_names_second = [name for name, _ in tmp_second]

        parsed_first_bak_max = max((value for _, value in bak_first), default=None)
        parsed_first_tmp_max = max((value for _, value in tmp_first), default=None)

        bak_new = [name for name in bak_names_second if name not in bak_names_first]
        tmp_new = [name for name in tmp_names_second if name not in tmp_names_first]
        _fail(failures, bool(bak_new), "016.18", "no fresh BAK directory name was created in the second replacement run")
        _fail(failures, bool(tmp_new), "016.19", "no fresh TMP directory name was created in the second staging run")

        if parsed_first_bak_max is not None:
            for name in bak_new:
                parsed = _parse_timestamp(name)
                if parsed is None:
                    continue
                _fail(
                    failures,
                    parsed > parsed_first_bak_max,
                    "016.18",
                    f"new BAK timestamp {name!r} was not greater than previous max {parsed_first_bak_max}",
                )
        if parsed_first_tmp_max is not None:
            for name in tmp_new:
                parsed = _parse_timestamp(name)
                if parsed is None:
                    continue
                _fail(
                    failures,
                    parsed > parsed_first_tmp_max,
                    "016.19",
                    f"new TMP timestamp {name!r} was not greater than previous max {parsed_first_tmp_max}",
                )


def check_log_output_timestamps(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_016_logs_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        peer = workspace / "peer"
        canon.mkdir()
        peer.mkdir()

        _write_text(canon / "log.txt", "log")
        _write_text(peer / "log.txt", "old")

        result = _run_kitchensync(["--verbosity", "trace", f"+{canon}", str(peer)], cwd=workspace)
        if not _assert_success(failures, "016.15", result, ["--verbosity", "trace", f"+{canon}", str(peer)]):
            return

        output = f"{result.stdout}\n{result.stderr}"
        matches = TIMESTAMP_RE.findall(output)
        _fail(failures, bool(matches), "016.15", "no timestamp-formatted values were emitted in trace logs")
        for token in matches:
            _fail(
                failures,
                _parse_timestamp(token) is not None,
                "016.15",
                f"log token {token!r} is not in required timestamp format",
            )


def main() -> int:
    failures: list[str] = []

    _fail(failures, WORKSPACE_ROOT.is_dir(), "precondition", f"workspace root missing: {WORKSPACE_ROOT}")
    _fail(failures, PROJECT_DIR.is_dir(), "precondition", f"project directory missing: {PROJECT_DIR}")
    _fail(failures, RELEASED_BINARY.is_file(), "precondition", f"released executable missing: {RELEASED_BINARY}")

    # not reasonably testable from CLI surface:
    # 016.1 -- exact xxHash64 seed-0 digest values are not independently verifiable without the same hash pipeline.
    # 016.6 -- proving the sentinel equals the digest of "/" is internal-only.

    if failures:
        print("FAIL: test_016_snapshot_paths_and_timestamps.py (precondition)")
        for index, failure in enumerate(failures, start=1):
            print(f"  {index:02d}. {failure}")
        return 1

    _run_case(
        "016.1/016.2/016.3/016.4/016.5/016.6/016.7",
        failures,
        lambda: check_snapshot_identifiers_and_parent_links(failures),
    )
    _run_case("016.4", failures, lambda: check_type_transition_reuses_snapshot_id(failures))
    _run_case(
        "016.8/016.9/016.10/016.11/016.12/016.20",
        failures,
        lambda: check_snapshot_timestamps_format_and_lexicographic_order(failures),
    )
    _run_case("016.17", failures, lambda: check_last_seen_refreshes_on_update(failures))
    _run_case(
        "016.13/016.14/016.16/016.18/016.19",
        failures,
        lambda: check_bak_tmp_timestamp_paths_and_freshness(failures),
    )
    _run_case("016.15", failures, lambda: check_log_output_timestamps(failures))

    if failures:
        print("FAIL: test_016_snapshot_paths_and_timestamps.py")
        for index, failure in enumerate(failures, start=1):
            print(f"  {index:02d}. {failure}")
        return 1

    print("PASS: test_016_snapshot_paths_and_timestamps.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
