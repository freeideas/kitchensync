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
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE_ROOT = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync")
PROJECT_DIR = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\proj")
WINDOWS_EXE_PATH = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\released\\kitchensync.exe")
POSIX_EXE_PATH = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\released\\kitchensync")
RELEASED_EXE_PATH = WINDOWS_EXE_PATH if os.name == "nt" else POSIX_EXE_PATH

SNAPSHOT_COLUMNS = [
    "id",
    "parent_id",
    "basename",
    "mod_time",
    "byte_size",
    "last_seen",
    "deleted_time",
]
BASE62_RE = re.compile(r"^[0-9A-Za-z]{11}$")
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")
TIMESTAMP_FMT = "%Y-%m-%d_%H-%M-%S_%fZ"


def _add_failure(failures: list[str], condition: bool, req: str, detail: str) -> None:
    if not condition:
        failures.append(f"{req}: {detail}")


def _run_kitchensync(args: Iterable[str], *, cwd: Path, timeout_seconds: float = 30.0) -> subprocess.CompletedProcess[str] | None:
    try:
        return subprocess.run(
            [str(RELEASED_EXE_PATH), *args],
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
            check=False,
        )
    except (FileNotFoundError, OSError) as exc:
        return subprocess.CompletedProcess(
            args=[str(RELEASED_EXE_PATH), *args],
            returncode=127,
            stdout="",
            stderr=f"failed to launch released executable: {exc}",
        )
    except subprocess.TimeoutExpired:
        return None


def _tracked_paths(peer_root: Path) -> set[str]:
    paths: set[str] = set()
    if not peer_root.exists():
        return paths

    for child in peer_root.rglob("*"):
        if any(part == ".kitchensync" for part in child.relative_to(peer_root).parts):
            continue
        relative = child.relative_to(peer_root)
        if str(relative) != ".":
            paths.add(relative.as_posix())
    return paths


def _load_snapshot(snapshot_path: Path | None) -> dict[str, object] | None:
    if snapshot_path is None or not snapshot_path.is_file():
        return None

    conn: sqlite3.Connection | None = None
    try:
        conn = sqlite3.connect(str(snapshot_path))
        conn.row_factory = sqlite3.Row

        journal_mode = str(conn.execute("PRAGMA journal_mode;").fetchone()[0]).lower()

        tables = [
            row["name"]
            for row in conn.execute(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name;"
            ).fetchall()
        ]

        views = [
            row["name"]
            for row in conn.execute(
                "SELECT name FROM sqlite_master WHERE type='view' ORDER BY name;"
            ).fetchall()
        ]

        columns = list(conn.execute("PRAGMA table_info(snapshot);").fetchall())
        rows = [dict(row) for row in conn.execute("SELECT * FROM snapshot;").fetchall()]

        indexed_columns: set[str] = set()
        index_names = [
            row["name"]
            for row in conn.execute(
                "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='snapshot' AND name NOT LIKE 'sqlite_autoindex_%';"
            ).fetchall()
        ]
        for index_name in index_names:
            for idx in conn.execute(f"PRAGMA index_info('{index_name}');").fetchall():
                indexed_columns.add(idx["name"])

        return {
            "journal_mode": journal_mode,
            "tables": tables,
            "views": views,
            "columns": columns,
            "rows": rows,
            "indexed_columns": indexed_columns,
            "indexes": index_names,
        }
    except Exception:
        return None
    finally:
        if conn is not None:
            conn.close()


def _timestamp_ok(value: str | None) -> bool:
    if value is None:
        return False
    if not TIMESTAMP_RE.fullmatch(value):
        return False
    try:
        datetime.strptime(value, TIMESTAMP_FMT).replace(tzinfo=timezone.utc)
    except ValueError:
        return False
    return True


def _parse_timestamp(value: str) -> datetime:
    return datetime.strptime(value, TIMESTAMP_FMT).replace(tzinfo=timezone.utc)


def _build_path_map(rows: list[dict], failures: list[str], req: str) -> tuple[dict[str, dict[str, object]], str | None]:
    by_id = {row["id"]: row for row in rows}

    for row in rows:
        _add_failure(
            failures,
            row.get("id") is not None,
            "005.12",
            f"row has NULL id in {req}",
        )
        _add_failure(
            failures,
            row.get("parent_id") is not None,
            "005.16",
            f"row has NULL parent_id in {req}",
        )

    parent_ids = {
        row["parent_id"]
        for row in rows
        if row.get("parent_id") is not None and row.get("parent_id") not in by_id
    }
    _add_failure(
        failures,
        bool(parent_ids),
        "005.17",
        "failed to identify parent_id sentinel for path reconstruction",
    )
    sentinel: str | None = next(iter(parent_ids), None) if parent_ids else None

    if sentinel is not None and parent_ids:
        _add_failure(
            failures,
            len(parent_ids) == 1,
            "005.17",
            f"expected one parent_id sentinel, found {sorted(parent_ids)}",
        )

    memo: dict[str, str] = {}
    visiting: set[str] = set()

    def resolve(row_id: str) -> str | None:
        if row_id in memo:
            return memo[row_id]
        if row_id in visiting:
            _add_failure(
                failures,
                False,
                "005.10",
                "cycle detected in snapshot parent_id graph",
            )
            return None

        row = by_id.get(row_id)
        if row is None:
            _add_failure(
                failures,
                False,
                "005.10",
                f"row id {row_id!r} is referenced but missing",
            )
            return None

        parent_id = row.get("parent_id")
        basename = row.get("basename")

        if parent_id is None:
            return None
        if basename is None:
            return None

        visiting.add(row_id)
        if parent_id == sentinel:
            path = str(basename)
        elif parent_id in by_id:
            parent_path = resolve(parent_id)
            if parent_path is None:
                path = None
            else:
                path = f"{parent_path}/{basename}"
        else:
            _add_failure(
                failures,
                False,
                "005.17",
                f"parent_id {parent_id!r} for row {row_id!r} does not refer to another row or sentinel",
            )
            path = None

        visiting.remove(row_id)
        if path is None:
            return None
        memo[row_id] = path
        return path

    path_by_id: dict[str, dict[str, object]] = {}
    for row_id in by_id:
        path = resolve(row_id)
        if path is None:
            continue
        if path in (entry["path"] for entry in path_by_id.values()):
            _add_failure(
                failures,
                False,
                "005.10",
                f"multiple snapshot rows map to same reconstructed path {path!r}",
            )
            continue
        row = by_id[row_id]
        path_by_id[row_id] = {
            "path": path,
            "row": row,
        }

    return path_by_id, sentinel


def _validate_schema(snapshot: dict, failures: list[str], req_label: str) -> None:
    _add_failure(
        failures,
        snapshot["journal_mode"] == "delete",
        "005.5",
        f"{req_label}: expected rollback journal mode 'delete', got {snapshot['journal_mode']!r}",
    )

    _add_failure(
        failures,
        len(snapshot["tables"]) == 1,
        "005.6",
        f"{req_label}: expected exactly one non-internal table, got {snapshot['tables']}",
    )
    _add_failure(
        failures,
        snapshot["tables"] == ["snapshot"],
        "005.7",
        f"{req_label}: expected only snapshot table, got {snapshot['tables']}",
    )
    _add_failure(
        failures,
        not snapshot["views"],
        "005.8",
        f"{req_label}: found views in snapshot database {snapshot['views']}",
    )

    columns = snapshot["columns"]
    column_names = [row["name"] for row in columns]
    _add_failure(
        failures,
        column_names == SNAPSHOT_COLUMNS,
        "005.9",
        f"{req_label}: expected columns {SNAPSHOT_COLUMNS}, got {column_names}",
    )

    actual_types = {row["name"]: str(row["type"]).upper() for row in columns}
    expected_types = {
        "id": "TEXT",
        "parent_id": "TEXT",
        "basename": "TEXT",
        "mod_time": "TEXT",
        "byte_size": "INTEGER",
        "last_seen": "TEXT",
        "deleted_time": "TEXT",
    }
    req_for_col = {
        "id": "005.12",
        "parent_id": "005.15",
        "basename": "005.18",
        "mod_time": "005.21",
        "byte_size": "005.25",
        "last_seen": "005.29",
        "deleted_time": "005.33",
    }
    for name, expected in expected_types.items():
        _add_failure(
            failures,
            actual_types.get(name) == expected,
            req_for_col[name],
            f"{req_label}: column {name} expected SQL type {expected}, got {actual_types.get(name)}",
        )

    notnull = {row["name"]: bool(row["notnull"]) for row in columns}
    _add_failure(
        failures,
        notnull.get("basename", False),
        "005.19",
        f"{req_label}: basename is nullable",
    )
    _add_failure(
        failures,
        notnull.get("mod_time", False),
        "005.22",
        f"{req_label}: mod_time is nullable",
    )
    _add_failure(
        failures,
        notnull.get("byte_size", False),
        "005.26",
        f"{req_label}: byte_size is nullable",
    )

    pk_columns = [row["name"] for row in columns if row["pk"] == 1]
    _add_failure(
        failures,
        pk_columns == ["id"],
        "005.13",
        f"{req_label}: expected id as primary key, got {pk_columns}",
    )

    indexed_columns = snapshot["indexed_columns"]
    _add_failure(
        failures,
        "parent_id" in indexed_columns,
        "005.40",
        f"{req_label}: no index on parent_id",
    )
    _add_failure(
        failures,
        "last_seen" in indexed_columns,
        "005.41",
        f"{req_label}: no index on last_seen",
    )
    _add_failure(
        failures,
        "deleted_time" in indexed_columns,
        "005.42",
        f"{req_label}: no index on deleted_time",
    )

    return


def _validate_rows(
    snapshot: dict,
    peer_root: Path,
    failures: list[str],
    req_label: str,
    expected_live_paths: set[str],
) -> dict[str, dict[str, object]]:
    rows = snapshot["rows"]
    path_by_id, sentinel = _build_path_map(rows, failures, req_label)

    by_path: dict[str, dict[str, object]] = {}
    for entry in path_by_id.values():
        by_path[str(entry["path"])] = entry["row"]

    _add_failure(
        failures,
        "" not in by_path,
        "005.11",
        f"{req_label}: sync root appears as a snapshot row",
    )

    _add_failure(
        failures,
        len(by_path) == len(rows),
        "005.10",
        f"{req_label}: reconstructed path count {len(by_path)} does not match row count {len(rows)}",
    )

    for path in expected_live_paths:
        _add_failure(
            failures,
            path in by_path,
            "005.10",
            f"{req_label}: expected live path {path!r} missing from snapshot table",
        )

    for path, row in by_path.items():
        _add_failure(
            failures,
            BASE62_RE.fullmatch(str(row["id"])) is not None,
            "005.14",
            f"{req_label}: id {row['id']!r} is not 11-char base62",
        )
        _add_failure(
            failures,
            BASE62_RE.fullmatch(str(row["parent_id"])) is not None,
            "005.16",
            f"{req_label}: parent_id {row['parent_id']!r} is not 11-char base62",
        )

        basename = row["basename"]
        _add_failure(
            failures,
            isinstance(basename, str) and basename != "",
            "005.18",
            f"{req_label}: missing basename for path {path!r}",
        )
        _add_failure(
            failures,
            "/" not in basename and "\\" not in basename,
            "005.20",
            f"{req_label}: basename {basename!r} is not a final path component",
        )

        _add_failure(
            failures,
            _timestamp_ok(row["mod_time"]),
            "005.23",
            f"{req_label}: invalid mod_time format for path {path!r}: {row['mod_time']!r}",
        )

        if "/" not in path and sentinel is not None:
            _add_failure(
                failures,
                row["parent_id"] == sentinel,
                "005.17",
                f"{req_label}: top-level path {path!r} does not use root sentinel parent_id",
            )

        entry = peer_root / path
        if entry.exists():
            _add_failure(
                failures,
                row["deleted_time"] is None,
                "005.35",
                f"{req_label}: live entry {path!r} has tombstone",
            )
            _add_failure(
                failures,
                row["last_seen"] is not None,
                "005.31",
                f"{req_label}: live entry {path!r} has NULL last_seen",
            )
            if row["last_seen"] is not None:
                _add_failure(
                    failures,
                    _timestamp_ok(row["last_seen"]),
                    "005.30",
                    f"{req_label}: invalid last_seen for path {path!r}: {row['last_seen']!r}",
                )
            else:
                _add_failure(
                    failures,
                    False,
                    "005.32",
                    f"{req_label}: live entry {path!r} must have non-NULL last_seen",
                )

            fs_mtime = datetime.fromtimestamp(entry.stat().st_mtime, tz=timezone.utc)
            db_mtime = _parse_timestamp(row["mod_time"])
            _add_failure(
                failures,
                abs((fs_mtime - db_mtime).total_seconds()) <= 2.0,
                "005.24",
                f"{req_label}: mod_time mismatch for {path!r}; fs={fs_mtime.strftime(TIMESTAMP_FMT)} db={row['mod_time']!r}",
            )

            if entry.is_dir():
                _add_failure(
                    failures,
                    row["byte_size"] == -1,
                    "005.28",
                    f"{req_label}: directory path {path!r} must have byte_size -1",
                )
            else:
                _add_failure(
                    failures,
                    row["byte_size"] >= 0,
                    "005.27",
                    f"{req_label}: file path {path!r} must have non-negative byte_size",
                )
                _add_failure(
                    failures,
                    entry.stat().st_size == row["byte_size"],
                    "005.27",
                    f"{req_label}: byte_size mismatch for {path!r}; fs={entry.stat().st_size}, db={row['byte_size']}",
                )
        else:
            if row["last_seen"] is not None:
                _add_failure(
                    failures,
                    _timestamp_ok(row["last_seen"]),
                    "005.30",
                    f"{req_label}: invalid last_seen for path {path!r}: {row['last_seen']!r}",
                )
            _add_failure(
                failures,
                path not in expected_live_paths,
                "005.10",
                f"{req_label}: snapshot row {path!r} is not present on disk and is not expected as tombstone",
            )
            _add_failure(
                failures,
                row["deleted_time"] is not None,
                "005.36",
                f"{req_label}: missing path {path!r} has NULL deleted_time",
            )
            if row["deleted_time"] is not None:
                _add_failure(
                    failures,
                    _timestamp_ok(row["deleted_time"]),
                    "005.34",
                    f"{req_label}: invalid deleted_time for {path!r}: {row['deleted_time']!r}",
                )

    # not reasonably testable: exact xxHash64 digest derivation for ids (005.14) and parent ids (005.16)
    # not reasonably testable: exact sentinel value for "/" (005.17)
    # not reasonably testable: NULL->non-NULL last_seen transition for in-flight copy rows (005.32)

    return by_path


def _validate_peer_state(peer_root: Path, failures: list[str], req_label: str) -> Path | None:
    state_dir = peer_root / ".kitchensync"
    snapshot = state_dir / "snapshot.db"

    _add_failure(
        failures,
        snapshot.is_file(),
        "005.1",
        f"{req_label}: snapshot.db missing at {snapshot}",
    )
    if not snapshot.is_file():
        return None

    _add_failure(
        failures,
        state_dir.is_dir(),
        "005.2",
        f"{req_label}: peer state directory .kitchensync missing",
    )

    sidecars = [
        p.name
        for p in state_dir.iterdir()
        if p.is_file() and p.name != "snapshot.db" and p.name.startswith("snapshot.db")
    ]
    _add_failure(
        failures,
        not sidecars,
        "005.3",
        f"{req_label}: sidecar files present for snapshot.db: {sidecars}",
    )

    return snapshot


def main() -> int:
    failures: list[str] = []

    _add_failure(
        failures,
        RELEASED_EXE_PATH.is_file(),
        "precondition",
        f"released executable missing: {RELEASED_EXE_PATH}",
    )

    with tempfile.TemporaryDirectory(prefix="ks_005_snapshot_storage_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "peer_a"
        sink = workspace / "peer_b"
        canon.mkdir()
        sink.mkdir()

        (canon / "folder").mkdir()
        (canon / "folder" / "child.txt").write_text("payload", encoding="utf-8")
        (canon / "root.txt").write_text("root", encoding="utf-8")

        initial = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
        _add_failure(
            failures,
            initial is not None,
            "005.4",
            "initial sync timed out",
        )
        if initial is not None:
            _add_failure(
                failures,
                initial.returncode == 0,
                "005.4",
                f"initial sync exited {initial.returncode}",
            )

        canon_snapshot = _validate_peer_state(canon, failures, "peer_a")
        sink_snapshot = _validate_peer_state(sink, failures, "peer_b")

        canon_db = _load_snapshot(canon_snapshot)
        sink_db = _load_snapshot(sink_snapshot)

        _add_failure(
            failures,
            canon_db is not None,
            "005.4",
            f"failed to open peer_a snapshot DB at {canon_snapshot}",
        )
        _add_failure(
            failures,
            sink_db is not None,
            "005.4",
            f"failed to open peer_b snapshot DB at {sink_snapshot}",
        )

        if canon_db is not None:
            _validate_schema(canon_db, failures, "peer_a initial")
        if sink_db is not None:
            _validate_schema(sink_db, failures, "peer_b initial")

        live_a = _tracked_paths(canon)
        live_b = _tracked_paths(sink)

        canon_rows_initial = {
            row_path: row
            for row_path, row in _validate_rows(canon_db, canon, failures, "peer_a initial", live_a).items()
        } if canon_db is not None else {}
        _ = _validate_rows(sink_db, sink, failures, "peer_b initial", live_b) if sink_db is not None else {}

        tracked_row_root = canon_rows_initial.get("root.txt")

        (canon / "root.txt").unlink()

        after_delete = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
        _add_failure(
            failures,
            after_delete is not None,
            "005.37",
            "post-delete sync timed out",
        )
        if after_delete is not None:
            _add_failure(
                failures,
                after_delete.returncode == 0,
                "005.37",
                f"post-delete sync exited {after_delete.returncode}",
            )

        live_a_after = _tracked_paths(canon)
        canon_db_after = _load_snapshot(canon_snapshot)
        _add_failure(
            failures,
            canon_db_after is not None,
            "005.4",
            f"failed to reopen peer_a snapshot DB after delete at {canon_snapshot}",
        )

        if canon_db_after is not None:
            _validate_schema(canon_db_after, failures, "peer_a after delete")
            canon_rows_after = _validate_rows(canon_db_after, canon, failures, "peer_a after delete", live_a_after)
            tombstoned_row = canon_rows_after.get("root.txt")
            _add_failure(
                failures,
                tombstoned_row is not None,
                "005.37",
                "deleted path root.txt was not retained as a snapshot row",
            )
            if tracked_row_root is not None and tombstoned_row is not None:
                _add_failure(
                    failures,
                    tombstoned_row["deleted_time"] == tracked_row_root["last_seen"],
                    "005.38",
                    "deleted_time was not set from previous last_seen",
                )
                _add_failure(
                    failures,
                    tombstoned_row["deleted_time"] is not None,
                    "005.36",
                    "tombstone row for root.txt has NULL deleted_time",
                )

            reconfirm = _run_kitchensync([f"+{canon}", str(sink)], cwd=workspace)
            _add_failure(
                failures,
                reconfirm is not None,
                "005.39",
                "reconfirm sync timed out",
            )
            if reconfirm is not None:
                _add_failure(
                    failures,
                    reconfirm.returncode == 0,
                    "005.39",
                    f"reconfirm sync exited {reconfirm.returncode}",
                )

            canon_db_reconfirm = _load_snapshot(canon_snapshot)
            _add_failure(
                failures,
                canon_db_reconfirm is not None,
                "005.4",
                f"failed to reopen peer_a snapshot DB after reconfirm at {canon_snapshot}",
            )
            if canon_db_reconfirm is not None:
                _validate_schema(canon_db_reconfirm, failures, "peer_a reconfirm")
                canon_rows_reconfirm = _validate_rows(
                    canon_db_reconfirm,
                    canon,
                    failures,
                    "peer_a reconfirm",
                    _tracked_paths(canon),
                )
                reconfirm_row = canon_rows_reconfirm.get("root.txt")
                if tombstoned_row is not None and reconfirm_row is not None:
                    _add_failure(
                        failures,
                        reconfirm_row["deleted_time"] == tombstoned_row["deleted_time"],
                        "005.39",
                        "reconfirming absence changed existing tombstone timestamp",
                    )

    if failures:
        for failure in failures:
            print(failure, file=sys.stdout)
        print(f"test_005_snapshot_storage.py failed ({len(failures)} issues)")
        return 1

    print("test_005_snapshot_storage.py passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
