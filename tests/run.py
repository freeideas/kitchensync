#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""End-to-end scenario test runner for KitchenSync.

Implements scenarios S-01..S-29 from specs/SCENARIOS.md against the released
binary for the current platform. See specs/DEVELOPMENT.md for how that binary
is built (code/build.py).

Usage:
    uv run tests/run.py               # run every scenario
    uv run tests/run.py S-02 S-04     # run a subset

The binary location can be overridden with the KITCHENSYNC_BIN environment
variable; otherwise it is looked up under <repo root>/released/.
"""

import gzip
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timedelta, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Exact contents of the fenced code block in specs/help.md, byte for byte,
# including its single trailing newline.
HELP_TEXT = 'Usage: kitchensync [options] <peer> <peer> [<peer>...]\n\nSynchronize file trees across multiple peers.\n\nRunning with no arguments (or --help, -h, /?) prints this help. See the specs for full behavior.\n\nPeers:\n  /path or c:\\path                 Local path (same as file://)\n  sftp://user@host/path            Remote over SSH\n  sftp://user@host:port/path       Non-standard SSH port\n  sftp://host/path                 Remote over SSH, current OS user\n  sftp://user:password@host/path   Inline password (prefer SSH keys)\n\nPrefix modifiers:\n  +<peer>                          Canon - this peer\'s state wins all conflicts\n  -<peer>                          Subordinate - overwritten to match the group\n\nFallback URLs (multiple paths to the same data):\n  [url1,url2,...]                  Try in order, first that connects wins\n  +[url1,url2,...]                 Canon peer with fallbacks\n  -[url1,url2,...]                 Subordinate peer with fallbacks\n\nPer-URL settings (query string, inside quotes):\n  "sftp://host/path?timeout-conn=60"     Connection timeout for this URL\n  "sftp://host/path?timeout-idle=10"     SFTP idle keep-alive TTL for this URL\n  "sftp://host/path?timeout-conn=60&timeout-idle=10"  Combine multiple\n\nOptions:\n  --dry-run          Read-only and plan, but make no peer changes\n  --parallel N       Files copied at the same time (default: 5)\n  --retries-copy N   Give up copying after this many tries (default: 3)\n  --retries-list N   Give up listing after this many tries (default: 3)\n  --timeout-conn N   SSH handshake timeout in seconds (default: 30)\n  --timeout-idle N   SFTP idle keep-alive TTL in seconds (default: 30)\n  --verbosity LEVEL  Verbosity: error, info, debug, trace (default: info)\n  --rollback TS      Roll the given peers back to timestamp TS, then exit\n  --undo             Roll the given peers back to just before their newest run\n  -x PATTERN         Exclude like .gitignore (or @file of patterns); repeatable\n  --keep-bak-days N  Delete displaced files (BAK/) after N days (default: 90)\n  --keep-del-days N  Forget deletion records after N days (default: 180)\n\nQuick start:\n  kitchensync +c:/photos sftp://user@host/photos      First sync (c: is canon)\n  kitchensync c:/photos sftp://host/photos            Bidirectional\n  kitchensync c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate\n  kitchensync c:/photos "sftp://user:p%40ss@host/photos"  Inline password\n\nWithout + on a first sync, peers are merged both ways and nothing is deleted.\nUse + to make one peer\'s contents win instead.\n\nTip: if ssh user@host and cd /path works, sftp://user@host/path will too.\n\nDisplaced files are recoverable from the sync root:\n  .kitchensync/BAK/<timestamp>/ (kept for --keep-bak-days days).\n'

BINARY: Path


class ScenarioFailure(Exception):
    """Raised with a human-readable diff describing why a scenario failed."""


def expect(condition: bool, message: str) -> None:
    if not condition:
        raise ScenarioFailure(message)


def locate_binary() -> Path:
    override = os.environ.get("KITCHENSYNC_BIN")
    if override:
        path = Path(override)
        if not path.is_file():
            print(
                f"KITCHENSYNC_BIN is set to {path} but no file exists there.",
                file=sys.stderr,
            )
            sys.exit(1)
        return path

    system = platform.system()
    if system == "Windows":
        name = "kitchensync.exe"
    elif system == "Darwin":
        name = "kitchensync.mac"
    elif system == "Linux":
        name = "kitchensync.linux"
    else:
        print(f"Unsupported platform: {system}", file=sys.stderr)
        sys.exit(1)

    path = REPO_ROOT / "released" / name
    if not path.is_file():
        print(
            f"Released binary not found at {path}.\n"
            "Build it first, e.g.: uv run code/build.py\n"
            "(see specs/DEVELOPMENT.md), or set KITCHENSYNC_BIN to point at "
            "an existing binary.",
            file=sys.stderr,
        )
        sys.exit(1)
    return path


def parse_spec_time(spec: str) -> float:
    """Parse a spec timestamp like '2024-01-01_12-00-00_000000Z' as UTC epoch seconds."""
    dt = datetime.strptime(spec, "%Y-%m-%d_%H-%M-%S_%fZ").replace(tzinfo=timezone.utc)
    return dt.timestamp()


def write_file(path: Path, data: bytes, mtime_str: str | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    if mtime_str is not None:
        epoch = parse_spec_time(mtime_str)
        os.utime(path, (epoch, epoch))


def run_ks(args: list[str], cwd: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(BINARY), *args],
        cwd=str(cwd),
        capture_output=True,
        timeout=120,
    )


def tree(root: Path) -> dict[str, bytes]:
    """Return relpath -> bytes for every regular file under root, ignoring .kitchensync/."""
    root = Path(root)
    result: dict[str, bytes] = {}
    if not root.exists():
        return result
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d != ".kitchensync"]
        for filename in filenames:
            full = Path(dirpath) / filename
            if full.is_symlink():
                continue
            rel = full.relative_to(root).as_posix()
            result[rel] = full.read_bytes()
    return result


def bak_contents(peer: Path) -> list[tuple[str, dict[str, bytes]]]:
    """Return [(timestamp_dir_name, {relpath: bytes})] for peer/.kitchensync/BAK/*/."""
    bak_root = Path(peer) / ".kitchensync" / "BAK"
    result: list[tuple[str, dict[str, bytes]]] = []
    if not bak_root.is_dir():
        return result
    for entry in sorted(bak_root.iterdir()):
        if not entry.is_dir():
            continue
        files: dict[str, bytes] = {}
        for dirpath, dirnames, filenames in os.walk(entry):
            dirnames[:] = [d for d in dirnames if d != ".kitchensync"]
            for filename in filenames:
                full = Path(dirpath) / filename
                rel = full.relative_to(entry).as_posix()
                files[rel] = full.read_bytes()
        if files:
            result.append((entry.name, files))
    return result


def merged_bak_files(peer: Path) -> dict[str, bytes]:
    """Flatten bak_contents(peer) across all timestamp dirs into one relpath -> bytes map."""
    merged: dict[str, bytes] = {}
    for _timestamp_dir, files in bak_contents(peer):
        merged.update(files)
    return merged


def check_file_bytes(path: Path, expected: bytes) -> None:
    expect(path.is_file(), f"{path}: expected a file with bytes {expected!r}, but it is missing")
    actual = path.read_bytes()
    expect(actual == expected, f"{path}: expected bytes {expected!r}, got {actual!r}")


def check_mtime(path: Path, spec: str) -> None:
    expected = parse_spec_time(spec)
    actual = path.stat().st_mtime
    diff = abs(actual - expected)
    expect(
        diff <= 1.0,
        f"{path}: expected mtime {spec} (epoch {expected}), got epoch {actual} (diff {diff}s)",
    )


def assert_result(
    result: subprocess.CompletedProcess,
    expected_stdout: bytes,
    expected_exit: int = 0,
    expected_stderr: bytes = b"",
) -> None:
    problems = []
    if result.returncode != expected_exit:
        problems.append(f"exit code: expected {expected_exit!r}, got {result.returncode!r}")
    if result.stdout != expected_stdout:
        problems.append(f"stdout: expected {expected_stdout!r}, got {result.stdout!r}")
    if result.stderr != expected_stderr:
        problems.append(f"stderr: expected {expected_stderr!r}, got {result.stderr!r}")
    if problems:
        raise ScenarioFailure("\n".join(problems))


# --------------------------------------------------------------------------
# Scenarios (specs/SCENARIOS.md S-01..S-29)
# --------------------------------------------------------------------------


def s01(tmp: Path) -> None:
    result = run_ks([], tmp)
    assert_result(result, HELP_TEXT.encode())
    for flag in ("--help", "-h", "/?"):
        result = run_ks([str(tmp / "a"), flag, str(tmp / "b")], tmp)
        assert_result(result, HELP_TEXT.encode())
    remaining = list(tmp.iterdir())
    expect(remaining == [], f"expected no filesystem changes, found {remaining}")


def s02(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "album" / "one.txt", b"canon\n", "2024-01-01_12-00-00_000000Z")
    peer_b.mkdir(parents=True, exist_ok=True)

    result = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp)
    assert_result(result, b"sync complete\n")

    check_file_bytes(peer_b / "album" / "one.txt", b"canon\n")
    check_mtime(peer_b / "album" / "one.txt", "2024-01-01_12-00-00_000000Z")
    for peer in (peer_a, peer_b):
        expect((peer / ".kitchensync" / "state.txt").is_file(), f"{peer}: missing .kitchensync/state.txt")
        expect(not (peer / "album" / ".kitchensync").exists(), f"{peer / 'album'}: should have no .kitchensync")


def s03(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "readme.txt", b"from A\n")
    write_file(peer_b / "other.txt", b"from B\n")

    result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    assert_result(
        result,
        b"first sync: no history found, merging both ways (nothing will be deleted); "
        b"use + to make one peer authoritative\nsync complete\n",
    )

    check_file_bytes(peer_a / "readme.txt", b"from A\n")
    check_file_bytes(peer_b / "readme.txt", b"from A\n")
    check_file_bytes(peer_a / "other.txt", b"from B\n")
    check_file_bytes(peer_b / "other.txt", b"from B\n")
    for peer in (peer_a, peer_b):
        expect((peer / ".kitchensync" / "state.txt").is_file(), f"{peer}: missing .kitchensync/state.txt")


def s04(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "report.txt", b"old\n", "2024-01-01_10-00-00_000000Z")
    peer_b.mkdir(parents=True, exist_ok=True)

    setup = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")

    write_file(peer_b / "report.txt", b"new\n", "2024-01-02_10-00-00_000000Z")

    result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    assert_result(result, b"sync complete\n")

    check_file_bytes(peer_a / "report.txt", b"new\n")
    check_mtime(peer_a / "report.txt", "2024-01-02_10-00-00_000000Z")
    check_file_bytes(peer_b / "report.txt", b"new\n")
    check_mtime(peer_b / "report.txt", "2024-01-02_10-00-00_000000Z")


def s05(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "old.txt", b"remove me\n", "2024-01-01_10-00-00_000000Z")
    peer_b.mkdir(parents=True, exist_ok=True)

    setup = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")

    (peer_a / "old.txt").unlink()

    result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    assert_result(result, b"sync complete\n")

    expect(not (peer_a / "old.txt").exists(), f"{peer_a / 'old.txt'} should not exist")
    expect(not (peer_b / "old.txt").exists(), f"{peer_b / 'old.txt'} should not exist")

    dirs = bak_contents(peer_b)
    expect(
        len(dirs) == 1,
        f"expected exactly one BAK timestamp dir under {peer_b}, found {[name for name, _ in dirs]}",
    )
    _timestamp_dir, files = dirs[0]
    expect(
        files == {"old.txt": b"remove me\n"},
        f"BAK dir contents mismatch under {peer_b}: {files}",
    )


def s06(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    peer_c = tmp / "C"
    write_file(peer_a / "shared.txt", b"group\n", "2024-01-01_10-00-00_000000Z")
    peer_b.mkdir(parents=True, exist_ok=True)

    setup = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")

    write_file(peer_c / "shared.txt", b"wrong\n")
    write_file(peer_c / "extra.txt", b"extra\n")

    result = run_ks(
        ["--verbosity", "error", str(peer_a), str(peer_b), f"-{peer_c}"], tmp
    )
    assert_result(result, b"sync complete\n")

    check_file_bytes(peer_c / "shared.txt", b"group\n")
    check_mtime(peer_c / "shared.txt", "2024-01-01_10-00-00_000000Z")
    expect(not (peer_c / "extra.txt").exists(), f"{peer_c / 'extra.txt'} should not exist")

    merged = merged_bak_files(peer_c)
    expect(
        merged == {"shared.txt": b"wrong\n", "extra.txt": b"extra\n"},
        f"BAK contents mismatch under {peer_c}: {merged}",
    )


def s07(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "keep.txt", b"copy\n")
    write_file(peer_a / "ignored" / "note.txt", b"do not copy\n")
    write_file(peer_b / "ignored" / "note.txt", b"leave alone\n")

    result = run_ks(
        ["--verbosity", "error", f"+{peer_a}", str(peer_b), "-x", "ignored"], tmp
    )
    assert_result(result, b"sync complete\n")

    check_file_bytes(peer_b / "keep.txt", b"copy\n")
    check_file_bytes(peer_b / "ignored" / "note.txt", b"leave alone\n")
    check_file_bytes(peer_a / "ignored" / "note.txt", b"do not copy\n")

    expect(
        merged_bak_files(peer_a) == {},
        f"no ignored/ entry should be displaced on {peer_a}: {merged_bak_files(peer_a)}",
    )
    expect(
        merged_bak_files(peer_b) == {},
        f"no ignored/ entry should be displaced on {peer_b}: {merged_bak_files(peer_b)}",
    )


def s08(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "dry.txt", b"plan only\n")
    peer_b.mkdir(parents=True, exist_ok=True)

    result = run_ks(
        ["--dry-run", "--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp
    )
    assert_result(result, b"dry run\nsync complete\n")

    expect(tree(peer_b) == {}, f"{peer_b}: expected no user files, found {tree(peer_b)}")
    for peer in (peer_a, peer_b):
        p = peer / ".kitchensync"
        expect(not p.exists(), f"{p} should not exist at all after a dry run")


def s09(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "item", b"file wins\n", "2024-01-01_10-00-00_000000Z")
    write_file(peer_b / "item" / "nested.txt", b"directory loses\n")

    result = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp)
    assert_result(result, b"sync complete\n")

    dest_item = peer_b / "item"
    expect(dest_item.is_file(), f"{dest_item} should be a file")
    check_file_bytes(dest_item, b"file wins\n")
    check_mtime(dest_item, "2024-01-01_10-00-00_000000Z")

    dirs = bak_contents(peer_b)
    expect(
        len(dirs) == 1,
        f"expected exactly one BAK timestamp dir under {peer_b}, found {[name for name, _ in dirs]}",
    )
    _timestamp_dir, files = dirs[0]
    expect(
        files == {"item/nested.txt": b"directory loses\n"},
        f"BAK dir contents mismatch under {peer_b}: {files}",
    )


def s10(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    peer_c = tmp / "C"
    write_file(peer_a / "shared.txt", b"group\n", "2024-01-01_10-00-00_000000Z")
    peer_b.mkdir(parents=True, exist_ok=True)

    setup = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")

    write_file(peer_c / "shared.txt", b"wrong\n")
    write_file(peer_c / "extra.txt", b"extra\n")

    result = run_ks(
        ["--verbosity", "error", str(peer_a), str(peer_b), str(peer_c)], tmp
    )
    assert_result(result, b"sync complete\n")

    check_file_bytes(peer_c / "shared.txt", b"group\n")
    check_mtime(peer_c / "shared.txt", "2024-01-01_10-00-00_000000Z")
    expect(not (peer_c / "extra.txt").exists(), f"{peer_c / 'extra.txt'} should not exist")

    merged = merged_bak_files(peer_c)
    expect(
        merged == {"shared.txt": b"wrong\n", "extra.txt": b"extra\n"},
        f"BAK contents mismatch under {peer_c}: {merged}",
    )
    expect((peer_c / ".kitchensync" / "state.txt").is_file(), f"{peer_c}: missing .kitchensync/state.txt")


ROLLBACK_HINT_RE = re.compile(
    r"^undo later with: kitchensync --rollback \d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z (.+)$"
)


def s11(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "note.txt", b"copy me\n", "2024-01-01_10-00-00_000000Z")
    peer_b.mkdir(parents=True, exist_ok=True)

    result = run_ks(["--verbosity", "info", f"+{peer_a}", str(peer_b)], tmp)
    expect(result.returncode == 0, f"exit code: expected 0, got {result.returncode!r}")
    expect(result.stderr == b"", f"stderr: expected empty, got {result.stderr!r}")

    stdout = result.stdout.decode("utf-8", errors="replace")
    lines = stdout.splitlines()
    expect(
        len(lines) == 3,
        f"stdout: expected exactly 3 lines, got {len(lines)}: {lines!r} (raw: {result.stdout!r})",
    )

    match = ROLLBACK_HINT_RE.match(lines[0])
    expect(
        match is not None,
        f"first stdout line does not match the rollback hint pattern {ROLLBACK_HINT_RE.pattern!r}: {lines[0]!r}",
    )
    actual_peers = match.group(1)
    # Path.as_uri() gives file:///tmp/x on POSIX and file:///C:/x on Windows,
    # which is how the binary prints local peers.
    expected_peers = f"{peer_a.as_uri()} {peer_b.as_uri()}"
    expect(
        actual_peers == expected_peers,
        "rollback hint peers part mismatch: expected "
        f"{expected_peers!r}, got {actual_peers!r} - if this is a reasonable "
        "alternate local-peer display form, reconcile it with the spec's wording "
        "rather than assuming a bug",
    )

    expect(lines[1] == "1C2 note.txt", f"second stdout line: expected '1C2 note.txt', got {lines[1]!r}")
    expect(lines[2] == "sync complete", f"third stdout line: expected 'sync complete', got {lines[2]!r}")

    check_file_bytes(peer_b / "note.txt", b"copy me\n")
    check_mtime(peer_b / "note.txt", "2024-01-01_10-00-00_000000Z")


def s12(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "b" / "c" / "one.txt", b"one\n", "2024-01-01_10-00-00_000000Z")
    (peer_b / "b" / "c").mkdir(parents=True, exist_ok=True)

    setup = run_ks(
        ["--verbosity", "error", f"+{peer_a / 'b' / 'c'}", str(peer_b / "b" / "c")],
        tmp,
    )
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")

    write_file(peer_a / "b" / "two.txt", b"two\n")
    (peer_a / "b" / "c" / "one.txt").unlink()

    result = run_ks(["--verbosity", "error", str(peer_a / "b"), str(peer_b / "b")], tmp)
    assert_result(
        result,
        b"first sync: no history found, merging both ways (nothing will be deleted); "
        b"use + to make one peer authoritative\nsync complete\n",
    )

    check_file_bytes(peer_b / "b" / "two.txt", b"two\n")
    expect(
        not (peer_b / "b" / "c" / "one.txt").exists(),
        f"{peer_b / 'b' / 'c' / 'one.txt'} should not exist",
    )

    merged = merged_bak_files(peer_b / "b")
    expect(merged == {"c/one.txt": b"one\n"}, f"files in {peer_b / 'b'}'s BAK: {merged}")
    nested = gzip.decompress((peer_b / "b" / "c" / ".kitchensync" / "state.gz").read_bytes()).decode()
    live = [line for line in nested.splitlines() if line.startswith("one.txt\t") and line.endswith("\t-")]
    expect(not live, f"{peer_b / 'b' / 'c'}'s state still lists one.txt as live: {nested!r}")


def s13(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "mine.txt", b"mine\n")
    write_file(peer_b / "theirs.txt", b"theirs\n")

    setup = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")
    expect(
        tree(peer_a) == {"mine.txt": b"mine\n", "theirs.txt": b"theirs\n"},
        f"setup: expected both files on {peer_a}, found {tree(peer_a)}",
    )
    expect(
        tree(peer_b) == {"mine.txt": b"mine\n", "theirs.txt": b"theirs\n"},
        f"setup: expected both files on {peer_b}, found {tree(peer_b)}",
    )

    result = run_ks(["--verbosity", "error", "--undo", str(peer_a), str(peer_b)], tmp)
    assert_result(result, b"rollback complete\n")

    expect(
        tree(peer_a) == {"mine.txt": b"mine\n"},
        f"{peer_a}: expected only mine.txt after undo, found {tree(peer_a)}",
    )
    expect(
        tree(peer_b) == {"theirs.txt": b"theirs\n"},
        f"{peer_b}: expected only theirs.txt after undo, found {tree(peer_b)}",
    )

    merged_a = merged_bak_files(peer_a)
    expect(
        merged_a.get("theirs.txt") == b"theirs\n",
        f"expected theirs.txt with bytes b'theirs\\n' under {peer_a}/.kitchensync/BAK/*: {merged_a}",
    )
    merged_b = merged_bak_files(peer_b)
    expect(
        merged_b.get("mine.txt") == b"mine\n",
        f"expected mine.txt with bytes b'mine\\n' under {peer_b}/.kitchensync/BAK/*: {merged_b}",
    )


def s14(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    for rel in (".DS_Store", "sub/.DS_Store", "sub/Thumbs.db"):
        write_file(peer_a / rel, b"x\n")
    write_file(peer_a / "keep.txt", b"k\n")
    write_file(peer_a / "sub" / "note.tmp", b"t\n")
    write_file(peer_a / "other.tmp", b"t2\n")
    write_file(peer_a / ".kitchensync" / "ignore", b"*.tmp\n!other.tmp\n")
    myignore = tmp / "myignore"
    write_file(myignore, b"# my file\n!.DS_Store\n")
    peer_b.mkdir(parents=True, exist_ok=True)

    result = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b), "-x", f"@{myignore}", "-x", "sub/"], tmp)
    assert_result(result, b"sync complete\n")
    expect(
        tree(peer_b) == {".DS_Store": b"x\n", "keep.txt": b"k\n", "other.tmp": b"t2\n"},
        f"{peer_b}: unexpected user files {tree(peer_b)}",
    )
    expect(not (peer_b / "sub").exists(), f"{peer_b / 'sub'} should not exist")


def s15(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "old" / "movie.txt", b"movie\n", "2024-01-01_10-00-00_000000Z")
    write_file(peer_a / "old" / "thumbs" / "1.txt", b"thumb\n", "2024-01-01_10-00-00_000000Z")
    peer_b.mkdir(parents=True, exist_ok=True)

    setup = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")

    (peer_b / "old").rename(peer_b / "new")

    result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    assert_result(result, b"sync complete\n")

    want = {"new/movie.txt": b"movie\n", "new/thumbs/1.txt": b"thumb\n"}
    for peer in (peer_a, peer_b):
        got = tree(peer)
        expect(got == want, f"user files under {peer} mismatch: {sorted(got)}")
        expect(not (peer / "old").exists(), f"{peer / 'old'} should not exist")


def s16(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    nfc = "caf\u00e9.txt"
    nfd = "cafe\u0301.txt"
    write_file(peer_a / nfc, b"same\n", "2024-01-01_10-00-00_000000Z")
    write_file(peer_b / nfd, b"same\n", "2024-01-01_10-00-00_000000Z")
    if len(os.listdir(peer_b)) != 1 or os.listdir(peer_b)[0] != nfd:
        return  # this filesystem normalizes names itself; nothing to test

    first = b"first sync: no history found, merging both ways (nothing will be deleted); use + to make one peer authoritative\n"
    for expected in (first + b"sync complete\n", b"sync complete\n"):
        result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
        assert_result(result, expected)
        for peer, name in ((peer_a, nfc), (peer_b, nfd)):
            got = tree(peer)
            expect(got == {name: b"same\n"}, f"user files under {peer} mismatch: {[ascii(n) for n in got]}")
            expect(not merged_bak_files(peer), f"unexpected BAK files under {peer}: {[ascii(n) for n in merged_bak_files(peer)]}")


def s17(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    when = "2024-01-01_10-00-00_000000Z"
    write_file(peer_a / "keep.txt", b"keep\n", when)
    write_file(peer_a / "movie.bin", b"0123456789\n", when)
    write_file(peer_b / "keep.txt", b"keep\n", when)

    setup = run_ks(["--verbosity", "error", str(peer_a), str(peer_b), "-x", "movie.bin"], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")
    write_file(peer_b / ".kitchensync" / "SWAP" / "movie.bin" / "new", b"01234")

    result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    assert_result(result, b"sync complete\n")
    for peer in (peer_a, peer_b):
        check_file_bytes(peer / "movie.bin", b"0123456789\n")
        check_mtime(peer / "movie.bin", when)
    swap = peer_b / ".kitchensync" / "SWAP"
    expect(not swap.exists(), f"{swap} should not exist")


def s18(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "keep.txt", b"keep\n", "2024-01-01_10-00-00_000000Z")
    write_file(peer_b / "keep.txt", b"keep\n", "2024-01-01_10-00-00_000000Z")
    write_file(peer_a / "movie.bin", b"new\n", "2024-01-02_10-00-00_000000Z")
    write_file(peer_b / "movie.bin", b"old\n", "2024-01-01_10-00-00_000000Z")

    setup = run_ks(["--verbosity", "error", str(peer_a), str(peer_b), "-x", "movie.bin"], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")
    swap = peer_b / ".kitchensync" / "SWAP"
    write_file(swap / "movie.bin" / "._new", bytes(4096))
    write_file(swap / "._movie.bin", bytes(4096))

    result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    assert_result(result, b"sync complete\n")
    for peer in (peer_a, peer_b):
        check_file_bytes(peer / "movie.bin", b"new\n")
    expect(not swap.exists(), f"{swap} should not exist")


MOVIE = bytes(range(256)) * 8192  # 2 MiB
MOVIE_TIME = "2024-01-01_10-00-00_000000Z"


def moved_movie_setup(tmp: Path) -> tuple[Path, Path]:
    """S-19 setup: A and B share movie.bin, then A moves it to shows/film.bin."""
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "movie.bin", MOVIE, MOVIE_TIME)
    peer_b.mkdir(parents=True, exist_ok=True)
    setup = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")
    check_file_bytes(peer_b / "movie.bin", MOVIE)
    (peer_a / "shows").mkdir()
    os.rename(peer_a / "movie.bin", peer_a / "shows" / "film.bin")
    return peer_a, peer_b


def check_info_lines(result: subprocess.CompletedProcess, expected: list[str]) -> None:
    expect(result.returncode == 0, f"exit code: expected 0, got {result.returncode!r}, stderr {result.stderr!r}")
    expect(result.stderr == b"", f"stderr: expected empty, got {result.stderr!r}")
    lines = result.stdout.decode("utf-8", errors="replace").splitlines()
    expect(len(lines) >= 1 and ROLLBACK_HINT_RE.match(lines[0]) is not None, f"first line is not the rollback hint: {lines!r}")
    expect(lines[1:] == expected, f"stdout after the hint: expected {expected!r}, got {lines[1:]!r}")


def s19(tmp: Path) -> None:
    peer_a, peer_b = moved_movie_setup(tmp)
    result = run_ks(["--verbosity", "info", str(peer_a), str(peer_b)], tmp)
    check_info_lines(result, ["X2 movie.bin", "M2 shows/film.bin", "sync complete"])
    expect(tree(peer_b) == {"shows/film.bin": MOVIE}, f"user files under B mismatch: {sorted(tree(peer_b))}")
    check_mtime(peer_b / "shows" / "film.bin", MOVIE_TIME)
    leftovers = [k for k in merged_bak_files(peer_b) if k.endswith("movie.bin")]
    expect(not leftovers, f"movie.bin should have left B's BAK, found {leftovers}")


def s20(tmp: Path) -> None:
    peer_a, peer_b = moved_movie_setup(tmp)
    changed = bytearray(MOVIE)
    changed[1_048_576] ^= 0xFF
    write_file(peer_a / "shows" / "film.bin", bytes(changed), MOVIE_TIME)
    result = run_ks(["--verbosity", "info", str(peer_a), str(peer_b)], tmp)
    check_info_lines(result, ["X2 movie.bin", "1C2 shows/film.bin", "sync complete"])
    expect(tree(peer_b) == {"shows/film.bin": bytes(changed)}, f"user files under B mismatch: {sorted(tree(peer_b))}")
    expect(merged_bak_files(peer_b).get("movie.bin") == MOVIE, "B's BAK should hold the original movie.bin")


def s21(tmp: Path) -> None:
    peer_a, peer_b = moved_movie_setup(tmp)
    sync = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    assert_result(sync, b"sync complete\n")
    result = run_ks(["--verbosity", "error", "--undo", str(peer_b)], tmp)
    assert_result(result, b"rollback complete\n")
    expect(tree(peer_b) == {"movie.bin": MOVIE}, f"user files under B after undo: {sorted(tree(peer_b))}")
    check_mtime(peer_b / "movie.bin", MOVIE_TIME)


def meta_snapshot(peer: Path) -> dict[str, bytes]:
    """relpath -> bytes for every file under any .kitchensync/ directory, except the root run log."""
    out: dict[str, bytes] = {}
    for dirpath, _dirnames, filenames in os.walk(peer):
        if ".kitchensync" not in Path(dirpath).relative_to(peer).parts:
            continue
        for f in filenames:
            rel = (Path(dirpath) / f).relative_to(peer).as_posix()
            if rel != ".kitchensync/runs.txt":
                out[rel] = (Path(dirpath) / f).read_bytes()
    return out


def meta_dirs(peer: Path) -> list[str]:
    """Every .kitchensync directory under peer, other than the root's."""
    found = []
    for dirpath, dirnames, _filenames in os.walk(peer):
        rel = Path(dirpath).relative_to(peer)
        if ".kitchensync" in rel.parts:
            dirnames[:] = []
            continue
        if ".kitchensync" in dirnames and rel != Path("."):
            found.append(rel.as_posix())
    return found


def s22(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "top.txt", b"x\n", "2024-01-01_10-00-00_000000Z")
    write_file(peer_a / "sub" / "inner.txt", b"x\n", "2024-01-01_10-00-00_000000Z")
    peer_b.mkdir(parents=True, exist_ok=True)
    setup = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")
    before = {p: meta_snapshot(p) for p in (peer_a, peer_b)}
    runs_before = {p: (p / ".kitchensync" / "runs.txt").read_text().count("\n") for p in (peer_a, peer_b)}
    result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    assert_result(result, b"sync complete\n")
    for p in (peer_a, peer_b):
        after = meta_snapshot(p)
        changed = sorted(k for k in set(before[p]) | set(after) if before[p].get(k) != after.get(k))
        expect(not changed, f"{p}: metadata changed on an unchanged run: {changed}")
        runs = (p / ".kitchensync" / "runs.txt").read_text().count("\n")
        expect(runs == runs_before[p] + 1, f"{p}: runs.txt should gain one line, had {runs_before[p]}, now {runs}")
        expect(not meta_dirs(p), f"{p}: .kitchensync below the root: {meta_dirs(p)}")


def s23(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    write_file(peer_a / "zz" / "movie.bin", MOVIE, MOVIE_TIME)
    peer_b.mkdir(parents=True, exist_ok=True)
    setup = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stderr {setup.stderr!r}")
    (peer_a / "aa").mkdir()
    os.rename(peer_a / "zz" / "movie.bin", peer_a / "aa" / "film.bin")
    before = tree(peer_b)
    dry = run_ks(["--dry-run", "--verbosity", "info", str(peer_a), str(peer_b)], tmp)
    assert_result(dry, b"dry run\nM2 aa/film.bin\nsync complete\n")
    expect(tree(peer_b) == before, "the dry run changed B")
    result = run_ks(["--verbosity", "info", str(peer_a), str(peer_b)], tmp)
    check_info_lines(result, ["M2 aa/film.bin", "sync complete"])
    expect(tree(peer_b) == {"aa/film.bin": MOVIE}, f"user files under B mismatch: {sorted(tree(peer_b))}")
    check_mtime(peer_b / "aa" / "film.bin", MOVIE_TIME)
    expect((peer_b / "zz").is_dir() and not any((peer_b / "zz").iterdir()), "B/zz should be an empty directory")
    expect(not bak_contents(peer_b), f"B should have no BAK: {bak_contents(peer_b)}")


def s24(tmp: Path) -> None:
    peer_a, peer_b = moved_movie_setup(tmp)
    first = run_ks(["--verbosity", "error", str(peer_a), str(peer_b), "-x", "shows"], tmp)
    assert_result(first, b"sync complete\n")
    expect(merged_bak_files(peer_b).get("movie.bin") == MOVIE, "B's BAK should hold movie.bin after the first run")
    expect(not (peer_b / "shows").exists(), "B should have no shows yet")
    result = run_ks(["--verbosity", "info", str(peer_a), str(peer_b)], tmp)
    check_info_lines(result, ["M2 shows/film.bin", "sync complete"])
    expect(tree(peer_b) == {"shows/film.bin": MOVIE}, f"user files under B mismatch: {sorted(tree(peer_b))}")
    check_mtime(peer_b / "shows" / "film.bin", MOVIE_TIME)
    expect(not merged_bak_files(peer_b), f"B's BAK should hold no file: {sorted(merged_bak_files(peer_b))}")


def s25(tmp: Path) -> None:
    peer_a = tmp / "A"
    peer_b = tmp / "B"
    when = "2024-01-01_10-00-00_000000Z"
    seen = "2024-02-01_10-00-00_000000Z"
    for peer in (peer_a, peer_b):
        write_file(peer / "sub" / "keep.txt", b"keep\n", when)
        write_file(peer / ".kitchensync" / "manifest.txt", f"sub\td\t{when}\t-1\t{seen}\t-\t-\n".encode())
        write_file(
            peer / "sub" / ".kitchensync" / "manifest.txt",
            f"gone.txt\tf\t{when}\t5\t{seen}\t-\t-\nkeep.txt\tf\t{when}\t5\t{seen}\t-\t-\n".encode(),
        )
    write_file(peer_b / "sub" / "gone.txt", b"gone\n", when)
    day_ago = datetime.now(timezone.utc) - timedelta(days=1)
    stamp = day_ago.strftime("%Y-%m-%d_%H-%M-%S_%fZ")
    write_file(peer_b / "sub" / ".kitchensync" / "BAK" / stamp / "old.txt", b"old\n")
    write_file(peer_b / "sub" / ".kitchensync" / "BAK" / stamp / "._old.txt", bytes(4096))
    stamp2 = (day_ago + timedelta(microseconds=1)).strftime("%Y-%m-%d_%H-%M-%S_%fZ")
    write_file(
        peer_b / "sub" / ".kitchensync" / "BAK" / stamp2 / "manifest.txt",
        f"keep.txt\tf\t{when}\t5\t2024-01-02_10-00-00_000000Z\t-\t-\n".encode(),
    )
    result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    assert_result(result, b"sync complete\n")
    for peer in (peer_a, peer_b):
        expect(tree(peer) == {"sub/keep.txt": b"keep\n"}, f"user files under {peer}: {sorted(tree(peer))}")
        expect(not (peer / "sub" / ".kitchensync").exists(), f"{peer / 'sub'}/.kitchensync should be gone")
        expect((peer / ".kitchensync" / "state.txt").is_file(), f"{peer}: missing .kitchensync/state.txt")
    bak = dict(bak_contents(peer_b))
    expect(bak.get(stamp) == {"sub/old.txt": b"old\n"}, f"B's BAK: {bak}")
    expect(sorted(merged_bak_files(peer_b)) == ["sub/gone.txt", "sub/old.txt"], f"files in B's BAK: {bak}")
    expect(merged_bak_files(peer_b)["sub/gone.txt"] == b"gone\n", f"B's BAK should hold sub/gone.txt: {bak}")


def s26(tmp: Path) -> None:
    peer_a, peer_b, peer_c = tmp / "A", tmp / "B", tmp / "C"
    for peer in (peer_a, peer_b):
        write_file(peer / "one.txt", b"one\n", "2024-01-01_10-00-00_000000Z")
    newer = b"#\t99\t2024-01-02_10-00-00_000000Z\n"
    write_file(peer_b / ".kitchensync" / "state.txt", newer)
    peer_c.mkdir(parents=True, exist_ok=True)
    result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b), str(peer_c)], tmp)
    shown = peer_b.as_posix() if peer_b.as_posix().startswith("/") else "/" + peer_b.as_posix()
    expected = (
        f"peer unreachable: file://{shown}: io_error: state.txt is format 99, newer than this "
        "KitchenSync reads (format 2); use a newer KitchenSync\n"
        "first sync: no history found, merging both ways (nothing will be deleted); use + to make one peer authoritative\n"
        "sync complete\n"
    )
    assert_result(result, expected.encode())
    expect((peer_b / ".kitchensync" / "state.txt").read_bytes() == newer, "B's state.txt changed")
    expect(sorted(p.name for p in (peer_b / ".kitchensync").iterdir()) == ["state.txt"], "B gained files under .kitchensync")
    check_file_bytes(peer_c / "one.txt", b"one\n")


def s27(tmp: Path) -> None:
    peer_a, peer_b, peer_c = tmp / "A", tmp / "B", tmp / "C"
    write_file(peer_a / "fresh.txt", b"fresh\n")
    write_file(peer_a / "edited.txt", b"v1\n")
    peer_b.mkdir(parents=True, exist_ok=True)
    peer_c.mkdir(parents=True, exist_ok=True)
    setup = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b), str(peer_c)], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stdout {setup.stdout!r}")
    (peer_a / "fresh.txt").unlink()
    second = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    assert_result(second, b"sync complete\n")
    expect(not (peer_b / "fresh.txt").exists(), "B should have lost fresh.txt in the two-peer run")
    (peer_a / "edited.txt").unlink()
    future = datetime.now(timezone.utc) + timedelta(minutes=1)
    write_file(peer_c / "edited.txt", b"v2 edited\n", future.strftime("%Y-%m-%d_%H-%M-%S_%fZ"))
    result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b), str(peer_c)], tmp)
    assert_result(result, b"sync complete\n")
    for peer in (peer_a, peer_b, peer_c):
        expect(tree(peer) == {"edited.txt": b"v2 edited\n"}, f"user files under {peer}: {tree(peer)}")


def s28(tmp: Path) -> None:
    peer_a, peer_b = tmp / "A", tmp / "B"
    when = "2024-01-01_10-00-00_000000Z"
    for name in ("keep.txt", "gone.txt"):
        write_file(peer_a / "sub" / name, name.encode(), when)
    write_file(peer_a / "top.txt", b"top", when)
    peer_b.mkdir(parents=True, exist_ok=True)
    setup = run_ks(["--verbosity", "error", f"+{peer_a}", str(peer_b)], tmp)
    expect(setup.returncode == 0, f"setup sync failed: exit {setup.returncode}, stdout {setup.stdout!r}")
    (peer_a / "sub" / "gone.txt").unlink()
    write_file(peer_b / "sub" / "new.txt", b"new")
    result = run_ks(["--verbosity", "error", str(peer_a / "sub"), str(peer_b / "sub")], tmp)
    assert_result(result, b"sync complete\n")
    for peer in (peer_a, peer_b):
        expect(tree(peer) == {"top.txt": b"top", "sub/keep.txt": b"keep.txt", "sub/new.txt": b"new"}, f"user files under {peer}: {sorted(tree(peer))}")
        expect(not (peer / "sub" / ".kitchensync").exists(), f"{peer / 'sub'} should hold no .kitchensync")
    expect(merged_bak_files(peer_b) == {"sub/gone.txt": b"gone.txt"}, f"files in B's BAK: {merged_bak_files(peer_b)}")
    undo = run_ks(["--dry-run", "--verbosity", "info", "--undo", str(peer_b / "sub")], tmp)
    assert_result(undo, b"dry run\nR1 gone.txt\nrollback complete\n")
    before = {p: tree(p) for p in (peer_a, peer_b)}
    full = run_ks(["--verbosity", "info", str(peer_a), str(peer_b)], tmp)
    check_info_lines(full, ["sync complete"])
    expect({p: tree(p) for p in (peer_a, peer_b)} == before, "the full sync changed files the subfolder sync had settled")


def s29(tmp: Path) -> None:
    peer_a, peer_b = tmp / "A", tmp / "B"
    when, seen = "2024-01-01_10-00-00_000000Z", "2024-02-01_10-00-00_000000Z"
    write_file(peer_a / "keep.txt", b"keep", when)
    write_file(peer_b / "keep.txt", b"keep", when)
    write_file(peer_b / "gone.txt", b"gone", when)
    old_state = f"#\t{seen}\ngone.txt\tf\t{when}\t4\t{seen}\t-\nkeep.txt\tf\t{when}\t4\t{seen}\t-\n"
    for peer in (peer_a, peer_b):
        write_file(peer / ".kitchensync" / "state.txt", old_state.encode())
    result = run_ks(["--verbosity", "error", str(peer_a), str(peer_b)], tmp)
    assert_result(result, b"sync complete\n")
    for peer in (peer_a, peer_b):
        expect(tree(peer) == {"keep.txt": b"keep"}, f"user files under {peer}: {sorted(tree(peer))}")
        marker = (peer / ".kitchensync" / "state.txt").read_text()
        expect(marker.startswith("#\t2\t") and marker.count("\n") == 1, f"{peer}: state.txt should be a format-2 marker: {marker!r}")
        lines = gzip.decompress((peer / ".kitchensync" / "state.gz").read_bytes()).decode()
        expect("keep.txt\tf\t" in lines, f"{peer}: state.gz lacks keep.txt: {lines!r}")


SCENARIOS: list[tuple[str, str, "callable"]] = [
    ("S-01", "Help With No Arguments", s01),
    ("S-02", "First Sync From Canon", s02),
    ("S-03", "First Sync Without Canon Merges Both Ways", s03),
    ("S-04", "Bidirectional Sync Chooses Newer Modification Time", s04),
    ("S-05", "Deleted File Displaces Remaining Copies", s05),
    ("S-06", "Subordinate Peer Receives The Group Outcome", s06),
    ("S-07", "Command-Line Exclude Leaves Paths Untouched", s07),
    ("S-08", "Dry Run Does Not Change Peers", s08),
    ("S-09", "Canon File Replaces Directory Type Conflict", s09),
    ("S-10", "New Peer Without History Is Subordinate", s10),
    ("S-11", "Info Verbosity Emits The Rollback Hint And Copy Progress", s11),
    ("S-12", "Root Choice Does Not Matter", s12),
    ("S-13", "Undo Reverts A First-Sync Merge", s13),
    ("S-14", "Exclude Patterns, Ignore Files, And Negation", s14),
    ("S-15", "Folder Renamed On One Peer Is Not Copied Back", s15),
    ("S-16", "Names In Different Unicode Forms Are The Same File", s16),
    ("S-17", "An Interrupted Copy Is Not Put In Place", s17),
    ("S-18", "Mac Litter In SWAP Does Not Block Cleanup", s18),
    ("S-19", "A Moved File Is Moved, Not Copied Again", s19),
    ("S-20", "Same Size And Time But Different Content Is Copied", s20),
    ("S-21", "Undo Puts A Moved File Back", s21),
    ("S-22", "A Run Over An Unchanged Tree Writes No State", s22),
    ("S-23", "A Move Is Found Before The Walk Reaches The Old Path", s23),
    ("S-24", "A File Displaced In An Earlier Run Is Moved Into Place", s24),
    ("S-25", "Per-Directory Manifests Are Read And Converted", s25),
    ("S-26", "A Newer State Format Is Left Alone", s26),
    ("S-27", "A Deletion Wins Over The Same Version, Even Right After A Sync", s27),
    ("S-28", "Syncing A Subfolder Uses And Updates The Root's History", s28),
    ("S-29", "An Older State Format Is Read And Upgraded", s29),
]


def main(argv: list[str]) -> int:
    global BINARY
    BINARY = locate_binary()

    requested = set(argv)
    if requested:
        known_ids = {sid for sid, _title, _func in SCENARIOS}
        unknown = requested - known_ids
        if unknown:
            print(f"Unknown scenario id(s): {', '.join(sorted(unknown))}", file=sys.stderr)
            return 1
        to_run = [s for s in SCENARIOS if s[0] in requested]
    else:
        to_run = SCENARIOS

    passed = 0
    failed = 0
    for scenario_id, title, func in to_run:
        tmp_dir = tempfile.mkdtemp(prefix=f"kitchensync-{scenario_id}-")
        try:
            func(Path(tmp_dir))
        except ScenarioFailure as failure:
            failed += 1
            print(f"FAIL {scenario_id} {title}")
            for line in str(failure).splitlines():
                print(f"    {line}")
            print(f"    temp dir preserved: {tmp_dir}")
        except Exception as exc:  # noqa: BLE001 - surface any unexpected error as a failure
            failed += 1
            print(f"FAIL {scenario_id} {title}")
            print(f"    unexpected error: {exc!r}")
            print(f"    temp dir preserved: {tmp_dir}")
        else:
            passed += 1
            print(f"PASS {scenario_id} {title}")
            shutil.rmtree(tmp_dir, ignore_errors=True)

    print(f"{passed} passed, {failed} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
