#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import subprocess
import sys
import zipfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR  = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")

# Mandated verbatim output from specs/help.md (01.27).
# A verbatim match also satisfies 01.6 (URL forms), 01.7 (prefix modifiers),
# 01.8 (fallback bracket syntax), 01.9 (global flags + defaults), 01.25 (per-URL query settings).
MANDATED_HELP = r"""Usage: java -jar kitchensync.jar [options] <peer> <peer> [<peer>...]

Synchronize file trees across multiple peers.

Running with no arguments prints this help. See README.md for full docs.

Peers:
  /path or c:\path                 Local path (same as file://)
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


def run_cli(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), *args],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def check_help_output(failures: list[str], r: subprocess.CompletedProcess[str], label: str) -> None:
    """Assert exit 0, verbatim stdout, empty stderr for a help invocation."""
    if r.returncode != 0:
        failures.append(f"{label}: expected exit 0, got {r.returncode}")
    actual = r.stdout.replace("\r\n", "\n")
    if actual != MANDATED_HELP:
        first_diff = next(
            (i for i, (a, b) in enumerate(zip(actual, MANDATED_HELP)) if a != b),
            min(len(actual), len(MANDATED_HELP)),
        )
        ctx = slice(max(0, first_diff - 20), first_diff + 40)
        failures.append(
            f"{label}: stdout does not match mandated help (01.27)\n"
            f"  expected {len(MANDATED_HELP)} chars, got {len(actual)} chars\n"
            f"  first diff at char {first_diff}: "
            f"expected {MANDATED_HELP[ctx]!r}, got {actual[ctx]!r}"
        )
    if r.stderr.strip():
        failures.append(f"{label}: expected empty stderr (01.17), got {r.stderr[:200]!r}")


def check_jar_embeds_help(failures: list[str]) -> None:
    """Assert the released artifact contains the mandated help text."""
    expected = MANDATED_HELP.encode("utf-8")
    try:
        with zipfile.ZipFile(JAR) as jar:
            for entry in jar.infolist():
                if entry.is_dir():
                    continue
                if expected in jar.read(entry):
                    return
    except Exception as exc:
        failures.append(f"JAR embedded help text (01.28): could not inspect released artifact: {exc}")
        return

    failures.append("JAR embedded help text (01.28): mandated help text not found in released artifact")


def main() -> None:
    failures: list[str] = []

    # 01.1-01.4, 01.5, 01.17, 01.27: all four help invocations exit 0,
    # print the mandated text verbatim to stdout, and produce no stderr.
    for args, label in [
        ([], "no arguments (01.1)"),
        (["-h"], "-h (01.2)"),
        (["--help"], "--help (01.3)"),
        (["/?"], "'/?' (01.4)"),
    ]:
        check_help_output(failures, run_cli(*args), label)

    # 01.26: help flag in an otherwise-invalid invocation still exits 0 and prints help.
    for args, label in [
        (["-h",     "--zzz-unrecognized"], "-h with unrecognized flag (01.26)"),
        (["--help", "--zzz-unrecognized"], "--help with unrecognized flag (01.26)"),
        (["/?",     "--zzz-unrecognized"], "'/?' with unrecognized flag (01.26)"),
    ]:
        check_help_output(failures, run_cli(*args), label)

    # 01.28: the released JAR embeds the mandated help text.
    check_jar_embeds_help(failures)

    if failures:
        for msg in failures:
            print(f"FAIL: {msg}")
        sys.exit(1)

    print("All checks passed.")


if __name__ == "__main__":
    main()
