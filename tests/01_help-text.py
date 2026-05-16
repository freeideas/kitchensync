#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


PROJECT_DIR = Path(".")
JAVA = PROJECT_DIR / "tools/compiler/jdk/bin/java"
JAR = PROJECT_DIR / "released/kitchensync.jar"
WORK_DIR = PROJECT_DIR / "tests/.tmp/01_help-text"
ISOLATED_JAR = WORK_DIR / "isolated/kitchensync.jar"


EXPECTED_HELP = """Usage: java -jar kitchensync.jar [options] <peer> <peer> [<peer>...]

Synchronize file trees across multiple peers.

Running with no arguments prints this help. See README.md for full docs.

Peers:
  /path or c:\\path                 Local path (same as file://)
  sftp://user@host/path            Remote over SSH
  sftp://user@host:port/path       Non-standard SSH port
  sftp://host/path                 Remote over SSH, current OS user
  sftp://user:password@host/path   Inline password (prefer SSH keys)

Prefix modifiers:
  +<peer>                          Canon — this peer's state wins all conflicts
  -<peer>                          Subordinate — overwritten to match the group

Fallback URLs (multiple paths to the same data):
  [url1,url2,...]                  Try in order, first that connects wins
  +[url1,url2,...]                 Canon peer with fallbacks
  -[url1,url2,...]                 Subordinate peer with fallbacks

Per-URL settings (query string, inside quotes):
  "sftp://host/path?mc=5"          Max SFTP connections for this user+host+port
  "sftp://host/path?ct=60"         Connection timeout for this URL
  "sftp://host/path?ka=10"         SFTP idle keep-alive TTL for this URL
  "sftp://host/path?mc=5&ct=60"    Combine multiple

Options:
  -h, --help, /?                      Show this help
  --mc N             Max SFTP connections per user+host+port (default: 10)
  --ct N             SSH handshake timeout in seconds (default: 30)
  --ka N             SFTP idle keep-alive TTL in seconds (default: 30)
  -vl LEVEL          Verbosity level: error, info, debug, trace (default: info)
  --xd N             Delete stale TMP staging after N days (default: 2)
  --bd N             Delete displaced files (BAK/) after N days (default: 90)
  --td N             Forget deletion records after N days (default: 180)

Quick start:
  java -jar kitchensync.jar +c:/photos sftp://user@host/photos      First sync (c: is canon)
  java -jar kitchensync.jar c:/photos sftp://host/photos            Bidirectional
  java -jar kitchensync.jar c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate
  java -jar kitchensync.jar c:/photos "sftp://user:p%40ss@host/photos"  Inline password

Canon (+) is required on first sync when no peer has snapshot history.
After the first sync, bidirectional sync works without canon.

Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.

Displaced files are recoverable from nearby .kitchensync/BAK/ directories (kept for --bd days).
"""


@dataclass(frozen=True)
class Case:
    req_ids: tuple[str, ...]
    name: str
    args: tuple[str, ...]


HELP_CASES = [
    Case(("01.1", "01.5", "01.17", "01.27", "01.28"), "no arguments", ()),
    Case(("01.2", "01.5", "01.17", "01.27", "01.28"), "-h", ("-h",)),
    Case(("01.3", "01.5", "01.17", "01.27", "01.28"), "--help", ("--help",)),
    Case(("01.4", "01.5", "01.17", "01.27", "01.28"), "/?", ("/?",)),
    Case(
        ("01.26", "01.5", "01.17", "01.27", "01.28"),
        "-h before validation errors",
        ("--definitely-not-a-kitchensync-flag", "-h", "+peer-a", "+peer-b"),
    ),
    Case(
        ("01.26", "01.5", "01.17", "01.27", "01.28"),
        "--help before validation errors",
        ("--definitely-not-a-kitchensync-flag", "--help", "+peer-a", "+peer-b"),
    ),
    Case(
        ("01.26", "01.5", "01.17", "01.27", "01.28"),
        "/? before validation errors",
        ("--definitely-not-a-kitchensync-flag", "/?", "+peer-a", "+peer-b"),
    ),
]


REQUIRED_FRAGMENTS = [
    ("01.6", "/path or c:\\path"),
    ("01.6", "sftp://user@host/path"),
    ("01.6", "sftp://user@host:port/path"),
    ("01.6", "sftp://user:password@host/path"),
    ("01.7", "+<peer>"),
    ("01.7", "-<peer>"),
    ("01.8", "[url1,url2,...]"),
    ("01.9", "--mc N"),
    ("01.9", "default: 10"),
    ("01.9", "--ct N"),
    ("01.9", "default: 30"),
    ("01.9", "--ka N"),
    ("01.9", "-vl LEVEL"),
    ("01.9", "default: info"),
    ("01.9", "--xd N"),
    ("01.9", "default: 2"),
    ("01.9", "--bd N"),
    ("01.9", "default: 90"),
    ("01.9", "--td N"),
    ("01.9", "default: 180"),
    ("01.25", '"sftp://host/path?mc=5"'),
    ("01.25", '"sftp://host/path?ct=60"'),
    ("01.25", '"sftp://host/path?ka=10"'),
    ("01.25", '"sftp://host/path?mc=5&ct=60"'),
]


def req_label(req_ids: tuple[str, ...]) -> str:
    return ", ".join(req_ids)


def prepare_work_dir() -> None:
    if WORK_DIR.exists():
        shutil.rmtree(WORK_DIR)
    ISOLATED_JAR.parent.mkdir(parents=True)
    shutil.copy2(JAR, ISOLATED_JAR)


def run_cli(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA.resolve()), "-jar", str(ISOLATED_JAR.name), *args],
        cwd=ISOLATED_JAR.parent,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
        check=False,
    )


def first_difference(left: str, right: str) -> str:
    if left == right:
        return "no difference"
    limit = min(len(left), len(right))
    for index in range(limit):
        if left[index] != right[index]:
            return (
                f"first difference at byte/char {index}: "
                f"got {left[index:index + 40]!r}, expected {right[index:index + 40]!r}"
            )
    return f"length differs: got {len(left)}, expected {len(right)}"


def check_help_case(case: Case, failures: list[str]) -> str | None:
    try:
        result = run_cli(*case.args)
    except Exception as exc:
        failures.append(f"{req_label(case.req_ids)} {case.name}: command failed to run: {exc}")
        return None

    if result.returncode != 0:
        failures.append(
            f"{req_label(case.req_ids)} {case.name}: expected exit code 0, got {result.returncode}"
        )

    if result.stderr != "":
        failures.append(
            f"{req_label(case.req_ids)} {case.name}: expected empty stderr, got {result.stderr!r}"
        )

    if result.stdout != EXPECTED_HELP:
        failures.append(
            f"{req_label(case.req_ids)} {case.name}: stdout did not match mandated help text; "
            f"{first_difference(result.stdout, EXPECTED_HELP)}"
        )

    return result.stdout


def main() -> int:
    failures: list[str] = []
    prepare_work_dir()

    observed_help = None
    for case in HELP_CASES:
        stdout = check_help_case(case, failures)
        if observed_help is None and stdout:
            observed_help = stdout

    if observed_help:
        for req_id, fragment in REQUIRED_FRAGMENTS:
            if fragment not in observed_help:
                failures.append(f"{req_id}: help text missing required fragment {fragment!r}")
    else:
        failures.append("01.6, 01.7, 01.8, 01.9, 01.25: no help output was observed to check required content")

    if failures:
        print("FAIL tests/01_help-text.py")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print(f"PASS tests/01_help-text.py ({len(HELP_CASES)} invocations)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
