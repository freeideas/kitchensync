# /// script
# dependencies = [
#   "xxhash",
# ]
# ///

from __future__ import annotations

import datetime as dt
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import xxhash


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


LITERAL_WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
WORKSPACE_ROOT = (
    LITERAL_WORKSPACE_ROOT
    if LITERAL_WORKSPACE_ROOT.exists()
    else Path(__file__).resolve().parents[1]
)
LITERAL_EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")
KITCHENSYNC_EXE = (
    LITERAL_EXE if LITERAL_EXE.exists() else WORKSPACE_ROOT / "released" / "kitchensync.exe"
)

BASE62_ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
ROOT_PARENT_ID = "JyBskcNRrBK"
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")


class FailureCollector:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def equal(self, actual: object, expected: object, message: str) -> None:
        if actual != expected:
            self.failures.append(f"{message}: expected {expected!r}, got {actual!r}")


def base62_u64(value: int) -> str:
    if value == 0:
        encoded = "0"
    else:
        chars: list[str] = []
        while value:
            value, rem = divmod(value, 62)
            chars.append(BASE62_ALPHABET[rem])
        encoded = "".join(reversed(chars))
    return encoded.rjust(11, "0")


def path_id(relative_path: str) -> str:
    return base62_u64(xxhash.xxh64_intdigest(relative_path, seed=0))


def parent_id(relative_path: str) -> str:
    if "/" not in relative_path:
        return ROOT_PARENT_ID
    return path_id(relative_path.rsplit("/", 1)[0])


def parse_timestamp(value: str) -> dt.datetime:
    parsed = dt.datetime.strptime(value, "%Y-%m-%d_%H-%M-%S_%fZ")
    return parsed.replace(tzinfo=dt.timezone.utc)


def run_sync(args: list[str], failures: FailureCollector, label: str) -> subprocess.CompletedProcess[str]:
    try:
        result = subprocess.run(
            [str(KITCHENSYNC_EXE), *args],
            cwd=str(WORKSPACE_ROOT),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            shell=False,
            check=False,
        )
    except Exception as exc:
        failures.check(False, f"{label}: failed to launch released executable: {exc}")
        return subprocess.CompletedProcess([str(KITCHENSYNC_EXE), *args], 999, "", str(exc))

    failures.equal(result.returncode, 0, f"{label}: process exit code")
    failures.equal(result.stderr, "", f"{label}: stderr must be empty")
    failures.check(
        "sync complete" in result.stdout.splitlines(),
        f"{label}: stdout should include exact completion line",
    )
    return result


def snapshot_rows(peer_root: Path, failures: FailureCollector, label: str) -> dict[str, dict[str, object]]:
    db_path = peer_root / ".kitchensync" / "snapshot.db"
    failures.check(db_path.exists(), f"{label}: snapshot database should exist at {db_path}")
    if not db_path.exists():
        return {}

    try:
        with sqlite3.connect(str(db_path)) as conn:
            conn.row_factory = sqlite3.Row
            rows = conn.execute(
                """
                SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time
                FROM snapshot
                """
            ).fetchall()
    except sqlite3.Error as exc:
        failures.check(False, f"{label}: could not read snapshot database: {exc}")
        return {}

    return {str(row["id"]): dict(row) for row in rows}


def row_for_path(
    rows: dict[str, dict[str, object]],
    relative_path: str,
    failures: FailureCollector,
    label: str,
) -> dict[str, object]:
    expected_id = path_id(relative_path)
    row = rows.get(expected_id)
    failures.check(row is not None, f"{label}: expected snapshot row for {relative_path}")
    return row or {}


def validate_path_row(
    rows: dict[str, dict[str, object]],
    relative_path: str,
    basename: str,
    byte_size: int,
    failures: FailureCollector,
    label: str,
) -> None:
    row = row_for_path(rows, relative_path, failures, label)
    if not row:
        return
    failures.equal(row.get("id"), path_id(relative_path), f"{label}: id for {relative_path}")
    failures.equal(row.get("parent_id"), parent_id(relative_path), f"{label}: parent_id for {relative_path}")
    failures.equal(row.get("basename"), basename, f"{label}: basename for {relative_path}")
    failures.equal(row.get("byte_size"), byte_size, f"{label}: byte_size for {relative_path}")


def timestamp_columns(rows: dict[str, dict[str, object]]) -> list[str]:
    values: list[str] = []
    for row in rows.values():
        for column in ("mod_time", "last_seen", "deleted_time"):
            value = row.get(column)
            if value is not None:
                values.append(str(value))
    return values


def validate_timestamps(values: list[str], failures: FailureCollector, label: str) -> None:
    parsed_pairs: list[tuple[str, dt.datetime]] = []
    for value in values:
        failures.check(TIMESTAMP_RE.fullmatch(value) is not None, f"{label}: bad timestamp format {value!r}")
        try:
            parsed = parse_timestamp(value)
        except ValueError as exc:
            failures.check(False, f"{label}: timestamp {value!r} did not parse as UTC microseconds: {exc}")
            continue
        failures.equal(parsed.tzinfo, dt.timezone.utc, f"{label}: timestamp timezone for {value!r}")
        parsed_pairs.append((value, parsed))

    lexicographic = [value for value, _ in sorted(parsed_pairs, key=lambda item: item[0])]
    chronological = [value for value, _ in sorted(parsed_pairs, key=lambda item: item[1])]
    failures.equal(
        lexicographic,
        chronological,
        f"{label}: timestamp strings should sort in represented UTC time order",
    )


def list_bak_timestamp_dirs(peer_root: Path) -> list[str]:
    timestamps: list[str] = []
    for bak_dir in peer_root.rglob("BAK"):
        if bak_dir.is_dir() and bak_dir.parent.name == ".kitchensync":
            for child in bak_dir.iterdir():
                if child.is_dir():
                    timestamps.append(child.name)
    return timestamps


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def remove_path(path: Path) -> None:
    if path.is_dir():
        shutil.rmtree(path)
    elif path.exists():
        path.unlink()


def main() -> int:
    failures = FailureCollector()
    failures.check(path_id("/") == ROOT_PARENT_ID, "test oracle: root sentinel hash should match spec")
    failures.check(KITCHENSYNC_EXE.exists(), f"released executable should exist at {KITCHENSYNC_EXE}")

    with tempfile.TemporaryDirectory(prefix="kitchensync-008-") as tmp:
        root = Path(tmp)
        peer_a = root / "peer-a"
        peer_b = root / "peer-b"
        peer_a.mkdir()
        peer_b.mkdir()

        write_text(peer_a / "alpha.txt", "alpha\n")
        write_text(peer_a / "two-delete-a.txt", "delete a\n")
        write_text(peer_a / "two-delete-b.txt", "delete b\n")
        write_text(peer_a / "folder" / "child.txt", "child\n")
        write_text(peer_a / "gone_dir" / "child.txt", "gone child\n")

        first = run_sync([f"+{peer_a}", str(peer_b)], failures, "first sync")
        failures.check(
            not any(TIMESTAMP_RE.search(line) and TIMESTAMP_RE.fullmatch(line) is None for line in first.stdout.splitlines()),
            "first sync: any timestamp-like stdout token should use the required format",
        )

        rows_a_first = snapshot_rows(peer_a, failures, "peer A first")
        rows_b_first = snapshot_rows(peer_b, failures, "peer B first")

        for rows, label in ((rows_a_first, "peer A first"), (rows_b_first, "peer B first")):
            failures.check(
                ROOT_PARENT_ID not in rows,
                f"{label}: sync root sentinel must not be stored as a snapshot row",
            )
            validate_path_row(rows, "alpha.txt", "alpha.txt", len("alpha\n"), failures, label)
            validate_path_row(rows, "folder", "folder", -1, failures, label)
            validate_path_row(rows, "folder/child.txt", "child.txt", len("child\n"), failures, label)
            validate_path_row(rows, "gone_dir", "gone_dir", -1, failures, label)
            validate_path_row(rows, "gone_dir/child.txt", "child.txt", len("gone child\n"), failures, label)
            validate_timestamps(timestamp_columns(rows), failures, f"{label} database")

            last_seen_values = [
                str(row["last_seen"]) for row in rows.values() if row.get("last_seen") is not None
            ]
            failures.equal(
                len(last_seen_values),
                len(set(last_seen_values)),
                f"{label}: generated last_seen values should not be reused within one sync run",
            )

        old_alpha_seen_a = row_for_path(rows_a_first, "alpha.txt", failures, "peer A first").get("last_seen")
        old_alpha_seen_b = row_for_path(rows_b_first, "alpha.txt", failures, "peer B first").get("last_seen")
        old_gone_dir_seen_b = row_for_path(rows_b_first, "gone_dir", failures, "peer B first").get("last_seen")
        old_child_seen_b = row_for_path(rows_b_first, "gone_dir/child.txt", failures, "peer B first").get("last_seen")

        time.sleep(1.1)
        remove_path(peer_a / "alpha.txt")
        remove_path(peer_a / "two-delete-a.txt")
        remove_path(peer_a / "two-delete-b.txt")
        remove_path(peer_a / "gone_dir")

        second = run_sync([f"+{peer_a}", str(peer_b)], failures, "second sync")
        stdout_values = [
            token
            for line in second.stdout.splitlines()
            for token in line.split()
            if TIMESTAMP_RE.fullmatch(token)
        ]
        validate_timestamps(stdout_values, failures, "second sync stdout")

        rows_a_second = snapshot_rows(peer_a, failures, "peer A second")
        rows_b_second = snapshot_rows(peer_b, failures, "peer B second")
        alpha_a = row_for_path(rows_a_second, "alpha.txt", failures, "peer A second")
        alpha_b = row_for_path(rows_b_second, "alpha.txt", failures, "peer B second")
        failures.equal(
            alpha_a.get("deleted_time"),
            old_alpha_seen_a,
            "008.14 peer A: confirmed absent deleted_time should copy existing last_seen",
        )
        failures.equal(
            alpha_b.get("deleted_time"),
            old_alpha_seen_b,
            "008.14 peer B: displaced file deleted_time should copy existing last_seen",
        )

        gone_dir_b = row_for_path(rows_b_second, "gone_dir", failures, "peer B second")
        gone_child_b = row_for_path(rows_b_second, "gone_dir/child.txt", failures, "peer B second")
        failures.equal(
            gone_dir_b.get("deleted_time"),
            old_gone_dir_seen_b,
            "008.14 peer B: displaced directory deleted_time should copy existing last_seen",
        )
        failures.equal(
            gone_child_b.get("deleted_time"),
            old_gone_dir_seen_b,
            "008.15 peer B: cascade descendant should use displaced directory deletion estimate",
        )
        failures.check(
            gone_child_b.get("deleted_time") != old_child_seen_b,
            "008.15 peer B: descendant cascade should not use the descendant's own last_seen",
        )

        for rows, label in ((rows_a_second, "peer A second"), (rows_b_second, "peer B second")):
            validate_timestamps(timestamp_columns(rows), failures, f"{label} database")

        bak_timestamps = list_bak_timestamp_dirs(peer_b)
        validate_timestamps(bak_timestamps, failures, "peer B BAK directory names")
        failures.check(
            len(bak_timestamps) >= 2,
            "008.13 peer B: test setup should create multiple observable BAK timestamp directories",
        )
        failures.equal(
            len(bak_timestamps),
            len(set(bak_timestamps)),
            "008.13 peer B: generated BAK timestamp directory values should not be reused",
        )

        all_generated = [
            str(row["last_seen"])
            for rows in (rows_a_first, rows_b_first, rows_a_second, rows_b_second)
            for row in rows.values()
            if row.get("last_seen") is not None
        ] + bak_timestamps
        validate_timestamps(all_generated, failures, "all observed generated timestamps")

    # not reasonably testable: 008.13 TMP timestamp reuse, because successful
    # released runs clean temporary staging before the filesystem can observe it.

    if failures.failures:
        print("FAILURES:")
        for failure in failures.failures:
            print(f"- {failure}")
        return 1

    print("test_008_path_ids_and_timestamps passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
