# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

import datetime as dt
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

LITERAL_WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
WORKSPACE_ROOT = (
    LITERAL_WORKSPACE_ROOT
    if LITERAL_WORKSPACE_ROOT.exists()
    else Path(__file__).resolve().parents[1]
)
LITERAL_RELEASED_EXE = Path(
    "/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe"
)
RELEASED_EXE = (
    LITERAL_RELEASED_EXE
    if LITERAL_RELEASED_EXE.exists()
    else WORKSPACE_ROOT / "released" / "kitchensync.exe"
)

BASE62_ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
ROOT_PARENT_ID = "JyBskcNRrBK"
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")
MASK64 = (1 << 64) - 1

PRIME64_1 = 11400714785074694791
PRIME64_2 = 14029467366897019727
PRIME64_3 = 1609587929392839161
PRIME64_4 = 9650029242287828579
PRIME64_5 = 2870177450012600261


def rotl64(value, bits):
    return ((value << bits) | (value >> (64 - bits))) & MASK64


def xxh64_round(acc, lane):
    acc = (acc + lane * PRIME64_2) & MASK64
    acc = rotl64(acc, 31)
    return (acc * PRIME64_1) & MASK64


def xxh64_merge_round(acc, value):
    acc ^= xxh64_round(0, value)
    acc = (acc * PRIME64_1 + PRIME64_4) & MASK64
    return acc


def xxh64_avalanche(value):
    value ^= value >> 33
    value = (value * PRIME64_2) & MASK64
    value ^= value >> 29
    value = (value * PRIME64_3) & MASK64
    value ^= value >> 32
    return value & MASK64


def xxh64_seed0(text):
    data = text.encode("utf-8")
    length = len(data)
    offset = 0

    if length >= 32:
        v1 = (PRIME64_1 + PRIME64_2) & MASK64
        v2 = PRIME64_2
        v3 = 0
        v4 = (-PRIME64_1) & MASK64
        limit = length - 32
        while offset <= limit:
            v1 = xxh64_round(v1, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v2 = xxh64_round(v2, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v3 = xxh64_round(v3, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
            v4 = xxh64_round(v4, int.from_bytes(data[offset : offset + 8], "little"))
            offset += 8
        h64 = (
            rotl64(v1, 1) + rotl64(v2, 7) + rotl64(v3, 12) + rotl64(v4, 18)
        ) & MASK64
        h64 = xxh64_merge_round(h64, v1)
        h64 = xxh64_merge_round(h64, v2)
        h64 = xxh64_merge_round(h64, v3)
        h64 = xxh64_merge_round(h64, v4)
    else:
        h64 = PRIME64_5

    h64 = (h64 + length) & MASK64

    while offset + 8 <= length:
        lane = int.from_bytes(data[offset : offset + 8], "little")
        mixed = xxh64_round(0, lane)
        h64 ^= mixed
        h64 = (rotl64(h64, 27) * PRIME64_1 + PRIME64_4) & MASK64
        offset += 8

    if offset + 4 <= length:
        lane = int.from_bytes(data[offset : offset + 4], "little")
        h64 ^= (lane * PRIME64_1) & MASK64
        h64 = (rotl64(h64, 23) * PRIME64_2 + PRIME64_3) & MASK64
        offset += 4

    while offset < length:
        h64 ^= (data[offset] * PRIME64_5) & MASK64
        h64 = (rotl64(h64, 11) * PRIME64_1) & MASK64
        offset += 1

    return xxh64_avalanche(h64)


def path_id(relative_path):
    value = xxh64_seed0(relative_path)
    chars = []
    if value == 0:
        chars.append("0")
    else:
        while value:
            value, remainder = divmod(value, 62)
            chars.append(BASE62_ALPHABET[remainder])
    return "".join(reversed(chars)).rjust(11, "0")


def check(failures, condition, message):
    if not condition:
        failures.append(message)


def write_text(path, text, timestamp):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    ns = int(timestamp.timestamp() * 1_000_000_000)
    Path(path).touch()
    import os

    os.utime(path, ns=(ns, ns))


def run_kitchensync(args, failures, label):
    started = dt.datetime.now(dt.UTC)
    try:
        completed = subprocess.run(
            [str(RELEASED_EXE), *[str(arg) for arg in args]],
            cwd=str(WORKSPACE_ROOT),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=45,
            check=False,
        )
    except Exception as exc:
        failures.append(f"{label}: failed to launch kitchensync: {exc!r}")
        return None, started, dt.datetime.now(dt.UTC)

    finished = dt.datetime.now(dt.UTC)
    check(
        failures,
        completed.returncode == 0,
        f"{label}: expected exit code 0, got {completed.returncode}; "
        f"stdout={completed.stdout!r}; stderr={completed.stderr!r}",
    )
    check(failures, completed.stderr == "", f"{label}: expected empty stderr")
    return completed, started, finished


def read_snapshot(peer_root, failures, label):
    db_path = peer_root / ".kitchensync" / "snapshot.db"
    if not db_path.exists():
        failures.append(f"{label}: snapshot database was not created at {db_path}")
        return {}

    try:
        with sqlite3.connect(str(db_path)) as conn:
            conn.row_factory = sqlite3.Row
            rows = conn.execute(
                "SELECT id, parent_id, basename, mod_time, byte_size, "
                "last_seen, deleted_time FROM snapshot"
            ).fetchall()
    except Exception as exc:
        failures.append(f"{label}: could not read snapshot database: {exc!r}")
        return {}

    return {row["id"]: dict(row) for row in rows}


def parse_timestamp(value, failures, context):
    if value is None:
        failures.append(f"{context}: timestamp was NULL")
        return None
    if not TIMESTAMP_RE.match(value):
        failures.append(f"{context}: {value!r} did not match YYYY-MM-DD_HH-mm-ss_ffffffZ")
        return None
    try:
        parsed = dt.datetime.strptime(value, "%Y-%m-%d_%H-%M-%S_%fZ")
    except ValueError as exc:
        failures.append(f"{context}: could not parse timestamp {value!r}: {exc}")
        return None
    return parsed.replace(tzinfo=dt.UTC)


def collect_db_timestamps(rows):
    values = []
    for row in rows.values():
        for column in ("mod_time", "last_seen", "deleted_time"):
            value = row[column]
            if value is not None:
                values.append((value, f"{row['basename']}.{column}"))
    return values


def assert_timestamp_values(failures, values):
    parsed = []
    for value, context in values:
        parsed_value = parse_timestamp(value, failures, context)
        if parsed_value is not None:
            parsed.append((value, parsed_value))
    check(
        failures,
        [value for value, _ in sorted(parsed, key=lambda item: item[0])]
        == [value for value, _ in sorted(parsed, key=lambda item: item[1])],
        "timestamp strings did not sort in the same order as parsed UTC times",
    )


def assert_generated_timestamps_in_window(failures, rows, started, finished, label):
    lower = started - dt.timedelta(seconds=10)
    upper = finished + dt.timedelta(seconds=10)
    for row in rows.values():
        value = row["last_seen"]
        if value is None:
            continue
        parsed = parse_timestamp(value, failures, f"{label}:{row['basename']}.last_seen")
        if parsed is not None:
            check(
                failures,
                lower <= parsed <= upper,
                f"{label}:{row['basename']}.last_seen {value!r} was not in the UTC run window",
            )


def expected_paths():
    return {
        "docs": {"basename": "docs", "parent": "/", "byte_size": -1},
        "docs/readme.txt": {"basename": "readme.txt", "parent": "docs", "byte_size": 15},
        "docs/notes": {"basename": "notes", "parent": "docs", "byte_size": -1},
        "docs/notes/deep.txt": {
            "basename": "deep.txt",
            "parent": "docs/notes",
            "byte_size": 10,
        },
        "remove_me.txt": {"basename": "remove_me.txt", "parent": "/", "byte_size": 7},
        "gone_dir": {"basename": "gone_dir", "parent": "/", "byte_size": -1},
        "gone_dir/child.txt": {"basename": "child.txt", "parent": "gone_dir", "byte_size": 6},
        "gone_dir/nested": {"basename": "nested", "parent": "gone_dir", "byte_size": -1},
        "gone_dir/nested/deep.txt": {
            "basename": "deep.txt",
            "parent": "gone_dir/nested",
            "byte_size": 6,
        },
    }


def assert_path_ids(failures, rows, label):
    expected = expected_paths()
    check(
        failures,
        len(rows) == len(expected),
        f"{label}: expected exactly {len(expected)} snapshot rows, got {len(rows)}",
    )
    check(
        failures,
        ROOT_PARENT_ID == path_id("/"),
        "test reference hash for the root parent sentinel did not match the required value",
    )

    for row_id, row in rows.items():
        check(failures, len(row_id) == 11, f"{label}: id {row_id!r} was not 11 chars")
        check(
            failures,
            all(ch in BASE62_ALPHABET for ch in row_id),
            f"{label}: id {row_id!r} used characters outside the required base62 alphabet",
        )
        parent_id = row["parent_id"]
        check(
            failures,
            len(parent_id) == 11 and all(ch in BASE62_ALPHABET for ch in parent_id),
            f"{label}: parent_id {parent_id!r} was not an 11-character base62 value",
        )

    check(
        failures,
        ROOT_PARENT_ID not in rows,
        f"{label}: sync root sentinel appeared as a snapshot row id",
    )

    for relpath, expected_row in expected.items():
        row_id = path_id(relpath)
        row = rows.get(row_id)
        check(failures, row is not None, f"{label}: missing row id for {relpath}")
        if row is None:
            continue
        expected_parent = ROOT_PARENT_ID if expected_row["parent"] == "/" else path_id(expected_row["parent"])
        check(
            failures,
            row["basename"] == expected_row["basename"],
            f"{label}: {relpath} basename was {row['basename']!r}",
        )
        check(
            failures,
            row["parent_id"] == expected_parent,
            f"{label}: {relpath} parent_id was {row['parent_id']!r}, expected {expected_parent!r}",
        )
        check(
            failures,
            row["byte_size"] == expected_row["byte_size"],
            f"{label}: {relpath} byte_size was {row['byte_size']!r}",
        )


def assert_unique_last_seen(failures, rows, label):
    last_seen = [row["last_seen"] for row in rows.values() if row["last_seen"] is not None]
    check(
        failures,
        len(last_seen) == len(set(last_seen)),
        f"{label}: generated last_seen values were reused within one sync run",
    )


def find_bak_timestamp_dirs(peer_root):
    bak_root = peer_root / ".kitchensync" / "BAK"
    if not bak_root.exists():
        return []
    return [path for path in bak_root.iterdir() if path.is_dir()]


def main():
    failures = []

    check(failures, RELEASED_EXE.exists(), f"released executable not found: {RELEASED_EXE}")

    # not reasonably testable: 008.8 and 008.9 for TMP directory names because
    # successful local syncs clean temporary staging rather than leaving a
    # durable TMP directory to observe.
    # not reasonably testable: 008.8 for log output because the progress and
    # completion specs do not require any timestamp-bearing log line.
    # not reasonably testable: 008.11 as a total generated-timestamp order
    # because the product does not expose the call order of timestamp generation.

    file_time = dt.datetime(2023, 5, 6, 7, 8, 9, 123456, tzinfo=dt.UTC)

    with tempfile.TemporaryDirectory(prefix="kitchensync-008-") as temp_name:
        temp_root = Path(temp_name)
        peer_a = temp_root / "peer_a"
        peer_b = temp_root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        write_text(peer_a / "docs" / "readme.txt", "readme content\n", file_time)
        write_text(peer_a / "docs" / "notes" / "deep.txt", "deep note\n", file_time)
        write_text(peer_a / "remove_me.txt", "remove\n", file_time)
        write_text(peer_a / "gone_dir" / "child.txt", "child\n", file_time)
        write_text(peer_a / "gone_dir" / "nested" / "deep.txt", "nested", file_time)

        _first, first_started, first_finished = run_kitchensync(
            [f"+{peer_a}", peer_b], failures, "first sync"
        )

        rows_a_first = read_snapshot(peer_a, failures, "peer A after first sync")
        rows_b_first = read_snapshot(peer_b, failures, "peer B after first sync")
        if rows_a_first:
            assert_path_ids(failures, rows_a_first, "peer A after first sync")
            assert_unique_last_seen(failures, rows_a_first, "peer A after first sync")
            assert_generated_timestamps_in_window(
                failures, rows_a_first, first_started, first_finished, "peer A first sync"
            )
        if rows_b_first:
            assert_path_ids(failures, rows_b_first, "peer B after first sync")
            assert_unique_last_seen(failures, rows_b_first, "peer B after first sync")
            assert_generated_timestamps_in_window(
                failures, rows_b_first, first_started, first_finished, "peer B first sync"
            )

        readme_row = rows_a_first.get(path_id("docs/readme.txt"))
        if readme_row is not None:
            check(
                failures,
                readme_row["mod_time"] == "2023-05-06_07-08-09_123456Z",
                "file mod_time was not recorded as the expected UTC microsecond timestamp",
            )

        (peer_a / "remove_me.txt").unlink(missing_ok=True)
        shutil.rmtree(peer_a / "gone_dir")

        _second, second_started, second_finished = run_kitchensync(
            [f"+{peer_a}", peer_b], failures, "second sync"
        )

        rows_a_second = read_snapshot(peer_a, failures, "peer A after second sync")
        rows_b_second = read_snapshot(peer_b, failures, "peer B after second sync")

        remove_id = path_id("remove_me.txt")
        gone_dir_id = path_id("gone_dir")
        cascade_ids = [
            path_id("gone_dir"),
            path_id("gone_dir/child.txt"),
            path_id("gone_dir/nested"),
            path_id("gone_dir/nested/deep.txt"),
        ]

        if rows_a_first and rows_a_second and remove_id in rows_a_first and remove_id in rows_a_second:
            check(
                failures,
                rows_a_second[remove_id]["deleted_time"] == rows_a_first[remove_id]["last_seen"],
                "confirmed absent row did not copy its existing last_seen into deleted_time",
            )

        if rows_b_first and rows_b_second and remove_id in rows_b_first and remove_id in rows_b_second:
            check(
                failures,
                rows_b_second[remove_id]["deleted_time"] == rows_b_first[remove_id]["last_seen"],
                "displaced file row did not copy its existing last_seen into deleted_time",
            )

        if rows_b_first and rows_b_second and gone_dir_id in rows_b_first:
            deletion_estimate = rows_b_first[gone_dir_id]["last_seen"]
            for cascade_id in cascade_ids:
                row = rows_b_second.get(cascade_id)
                check(failures, row is not None, f"missing cascade row {cascade_id}")
                if row is not None:
                    check(
                        failures,
                        row["deleted_time"] == deletion_estimate,
                        f"cascade row {row['basename']!r} did not use the displaced directory deletion estimate",
                    )

        check(
            failures,
            not (peer_b / "remove_me.txt").exists(),
            "peer B still had remove_me.txt after canon deletion",
        )
        check(
            failures,
            not (peer_b / "gone_dir").exists(),
            "peer B still had gone_dir after canon directory deletion",
        )

        bak_timestamp_dirs = find_bak_timestamp_dirs(peer_b)
        check(failures, bak_timestamp_dirs, "peer B did not create any BAK timestamp directory")
        for bak_dir in bak_timestamp_dirs:
            parsed = parse_timestamp(bak_dir.name, failures, f"BAK directory {bak_dir.name}")
            if parsed is not None:
                check(
                    failures,
                    second_started - dt.timedelta(seconds=10)
                    <= parsed
                    <= second_finished + dt.timedelta(seconds=10),
                    f"BAK timestamp {bak_dir.name!r} was not in the UTC second-run window",
                )

        all_timestamp_values = []
        for rows in (rows_a_first, rows_b_first, rows_a_second, rows_b_second):
            all_timestamp_values.extend(collect_db_timestamps(rows))
        all_timestamp_values.extend((path.name, f"BAK directory {path.name}") for path in bak_timestamp_dirs)
        assert_timestamp_values(failures, all_timestamp_values)

    if failures:
        print("FAILURES:")
        for failure in failures:
            print(f"- {failure}")
        raise SystemExit(1)

    print("test_008_path_ids_and_timestamps passed")


if __name__ == "__main__":
    main()
