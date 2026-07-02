# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
from __future__ import annotations

import os
import re
import sqlite3
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


LITERAL_WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
WORKSPACE = LITERAL_WORKSPACE if LITERAL_WORKSPACE.exists() else Path(__file__).resolve().parents[1]
KITCHENSYNC = WORKSPACE / "released" / "kitchensync.exe"
TIMEOUT_SECONDS = 30
PROGRESS_RE = re.compile(r"^[CX] [^\r\n]+$")
TRACE_RE = re.compile(r"^copy-slots active=(\d+)/(\d+)$")
CONTROL_RE = re.compile(r"[\x1b\r]")


@dataclass
class RunResult:
    args: list[str]
    returncode: int
    stdout: str
    stderr: str


def main() -> int:
    failures: list[str] = []

    check_argument_error_stdout(failures)
    check_first_sync_message(failures)
    check_no_contributing_peer_message(failures)
    check_info_debug_progress_and_completion(failures)
    check_error_verbosity_suppresses_progress(failures)
    check_trace_copy_slot_logging(failures)

    # not reasonably testable: 019.6 -- every enumerated error includes
    # transfer, archive, displacement, staging, set_mod_time, and snapshot
    # upload failures that require sabotaging the filesystem or transport.
    # not reasonably testable: 019.7
    # not reasonably testable: 019.8
    # not reasonably testable: 019.9
    # not reasonably testable: 019.10
    # not reasonably testable: 019.11

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1
    return 0


def run_kitchensync(args: list[str], cwd: Path | None = None) -> RunResult:
    completed = subprocess.run(
        [str(KITCHENSYNC), *args],
        cwd=str(cwd) if cwd is not None else str(WORKSPACE),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=TIMEOUT_SECONDS,
        shell=False,
        check=False,
    )
    return RunResult(args, completed.returncode, completed.stdout, completed.stderr)


def check(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def lines(text: str) -> list[str]:
    return text.splitlines()


def progress_lines(text: str) -> list[str]:
    return [line for line in lines(text) if PROGRESS_RE.match(line)]


def non_progress_lines(text: str) -> list[str]:
    return [
        line
        for line in lines(text)
        if line.strip() and not PROGRESS_RE.match(line) and not TRACE_RE.match(line)
    ]


def check_common_output_rules(result: RunResult, failures: list[str], label: str) -> None:
    check(result.stderr == "", failures, f"{label}: stderr must be empty")
    check(
        not CONTROL_RE.search(result.stdout),
        failures,
        f"{label}: stdout must not contain terminal control characters or carriage returns",
    )


def check_argument_error_stdout(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks019-arg-", dir=None) as tmp:
        peer = Path(tmp) / "only-peer"
        result = run_kitchensync([str(peer)])

    check(result.returncode == 1, failures, "019.3: non-help validation error must exit 1")
    check_common_output_rules(result, failures, "019.1/019.2/019.3 argument validation")
    out_lines = lines(result.stdout)
    check(out_lines, failures, "019.3: validation error must write to stdout")
    check(
        "Usage: kitchensync [options] <peer> <peer> [<peer>...]" in result.stdout,
        failures,
        "019.3: validation error must be followed by help text on stdout",
    )
    if out_lines:
        check(
            not out_lines[0].startswith("Usage:"),
            failures,
            "019.3: validation output must start with the error message before help text",
        )


def check_first_sync_message(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks019-first-", dir=None) as tmp:
        root = Path(tmp)
        peer_a = root / "peer-a"
        peer_b = root / "peer-b"
        peer_a.mkdir()
        peer_b.mkdir()
        result = run_kitchensync([str(peer_a), str(peer_b)])

    check(result.returncode == 1, failures, "019.4: first sync without canon must exit 1")
    check_common_output_rules(result, failures, "019.4 first sync")
    check(
        "First sync? Mark the authoritative peer with a leading +" in result.stdout,
        failures,
        "019.4: first sync without canon must print the specified suggestion",
    )


def check_no_contributing_peer_message(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks019-nocontrib-", dir=None) as tmp:
        root = Path(tmp)
        peer_a = root / "peer-a"
        peer_b = root / "peer-b"
        peer_a.mkdir()
        peer_b.mkdir()
        write_snapshot_with_one_file(peer_a, "seed.txt")
        write_snapshot_with_one_file(peer_b, "seed.txt")
        result = run_kitchensync([f"-{peer_a}", f"-{peer_b}"])

    check(result.returncode == 1, failures, "019.5: no contributing peer must exit 1")
    check_common_output_rules(result, failures, "019.5 no contributing peer")
    check(
        "No contributing peer reachable - cannot make sync decisions" in result.stdout,
        failures,
        "019.5: no contributing peer must print the specified message",
    )


def check_info_debug_progress_and_completion(failures: list[str]) -> None:
    info = run_success_case("info")
    debug = run_success_case("debug")

    check(info.returncode == 0, failures, "019.30: info sync scenario must exit 0")
    check(debug.returncode == 0, failures, "019.26: debug sync scenario must exit 0")
    check_common_output_rules(info, failures, "019.12-019.21 info sync")
    check_common_output_rules(debug, failures, "019.26 debug sync")
    check(
        info.stdout == debug.stdout,
        failures,
        "019.26: --verbosity debug must match --verbosity info observably",
    )

    progress = progress_lines(info.stdout)
    expected = ["C alpha.txt", "C docs/readme.txt", "X obsolete.txt", "X oldtree"]
    check(
        progress == expected,
        failures,
        "019.12: progress lines must appear once each in the order the actions happen",
    )
    for item in expected:
        check(progress.count(item) == 1, failures, f"019.12-019.16: expected one progress line {item!r}")

    check(
        "C docs" not in progress and "C emptydir" not in progress and "X oldtree/leaf.txt" not in progress,
        failures,
        "019.16-019.19: directory creation, listing, snapshots, and directory children must not emit extra progress lines",
    )
    check(
        all(not line.startswith(("C .kitchensync", "X .kitchensync")) for line in progress),
        failures,
        "019.19-019.20: snapshot and BAK/TMP/SWAP cleanup must not emit C/X progress",
    )
    check(
        all(re.match(r"^[CX] [^\\/](?:.*[^/])?$", line) and "\\" not in line for line in progress),
        failures,
        "019.13: progress lines must use one action letter, one space, and slash-separated relative paths",
    )
    check(
        bool(non_progress_lines(info.stdout)),
        failures,
        "019.30: successful sync must emit a final completion message separate from progress",
    )


def check_error_verbosity_suppresses_progress(failures: list[str]) -> None:
    result = run_success_case("error")

    check(result.returncode == 0, failures, "019.25: error-verbosity sync scenario must exit 0")
    check_common_output_rules(result, failures, "019.23-019.25 error verbosity")
    check(
        progress_lines(result.stdout) == [],
        failures,
        "019.24/019.25: --verbosity error must suppress info-level C and X progress lines",
    )
    check(
        bool(non_progress_lines(result.stdout)),
        failures,
        "019.23/019.30: completion output must remain visible at --verbosity error",
    )


def check_trace_copy_slot_logging(failures: list[str]) -> None:
    result = run_success_case("trace", extra_args=["--max-copies", "2"])

    check(result.returncode == 0, failures, "019.27: trace sync scenario must exit 0")
    check_common_output_rules(result, failures, "019.27-019.29 trace sync")
    trace_values = []
    for line in lines(result.stdout):
        match = TRACE_RE.match(line)
        if match:
            trace_values.append((int(match.group(1)), int(match.group(2))))

    check(trace_values, failures, "019.27: --verbosity trace must include copy-slot events")
    check(
        all(maximum == 2 for _, maximum in trace_values),
        failures,
        "019.28: trace copy-slot events must use the configured max in active=<n>/<max>",
    )
    check(
        all(0 <= active <= maximum for active, maximum in trace_values),
        failures,
        "019.28: trace copy-slot active count must stay within 0 and max",
    )
    check(
        any(active > 0 for active, _ in trace_values) and any(active == 0 for active, _ in trace_values),
        failures,
        "019.27/019.28: trace output must show acquire and release copy-slot events",
    )
    check(
        progress_lines(result.stdout),
        failures,
        "019.23/019.24: trace verbosity must include cumulative info-level progress output",
    )


def run_success_case(verbosity: str, extra_args: list[str] | None = None) -> RunResult:
    with tempfile.TemporaryDirectory(prefix=f"ks019-{verbosity}-", dir=None) as tmp:
        root = Path(tmp)
        canon = root / "canon"
        peer_b = root / "peer-b"
        peer_c = root / "peer-c"
        build_success_tree(canon, peer_b, peer_c)
        args = ["--verbosity", verbosity]
        if extra_args:
            args.extend(extra_args)
        args.extend([f"+{canon}", str(peer_b), str(peer_c)])
        return run_kitchensync(args)


def build_success_tree(canon: Path, peer_b: Path, peer_c: Path) -> None:
    for peer in (canon, peer_b, peer_c):
        peer.mkdir(parents=True)

    write_text(canon / "alpha.txt", "alpha\n")
    write_text(canon / "docs" / "readme.txt", "readme\n")
    (canon / "emptydir").mkdir()

    for peer in (peer_b, peer_c):
        write_text(peer / "obsolete.txt", "remove me\n")
        write_text(peer / "oldtree" / "leaf.txt", "remove tree\n")
        (peer / ".kitchensync" / "BAK" / "2000-01-01_00-00-00_000000Z").mkdir(parents=True)
        (peer / ".kitchensync" / "TMP" / "2000-01-01_00-00-00_000000Z").mkdir(parents=True)

    set_mtime(canon / "alpha.txt", 1_700_000_000)
    set_mtime(canon / "docs" / "readme.txt", 1_700_000_010)
    for peer in (peer_b, peer_c):
        set_mtime(peer / "obsolete.txt", 1_600_000_000)
        set_mtime(peer / "oldtree" / "leaf.txt", 1_600_000_010)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def set_mtime(path: Path, value: int) -> None:
    os.utime(path, (value, value))


def write_snapshot_with_one_file(peer: Path, relpath: str) -> None:
    metadata = peer / ".kitchensync"
    metadata.mkdir(parents=True, exist_ok=True)
    db_path = metadata / "snapshot.db"
    if db_path.exists():
        db_path.unlink()
    connection = sqlite3.connect(str(db_path))
    try:
        connection.execute("PRAGMA journal_mode=DELETE")
        connection.execute(
            """
            CREATE TABLE snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT,
                deleted_time TEXT
            )
            """
        )
        connection.execute("CREATE INDEX snapshot_parent_id ON snapshot(parent_id)")
        connection.execute("CREATE INDEX snapshot_last_seen ON snapshot(last_seen)")
        connection.execute("CREATE INDEX snapshot_deleted_time ON snapshot(deleted_time)")
        basename = relpath.rsplit("/", 1)[-1]
        connection.execute(
            """
            INSERT INTO snapshot
            (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
            VALUES (?, ?, ?, ?, ?, ?, NULL)
            """,
            (
                "00000000001",
                "00000000000",
                basename,
                "2024-01-01_00-00-00_000000Z",
                4,
                "2024-01-01_00-00-00_000000Z",
            ),
        )
        connection.commit()
    finally:
        connection.close()


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.TimeoutExpired as exc:
        print(f"subprocess timed out after {exc.timeout} seconds: {exc.cmd}")
        raise SystemExit(1)
    except FileNotFoundError as exc:
        print(f"required executable not found: {exc}")
        raise SystemExit(1)
