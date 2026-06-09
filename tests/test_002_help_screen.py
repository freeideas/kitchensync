# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///

import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

EXECUTABLE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")

EXPECTED_HELP = (
    "Usage: kitchensync [options] <peer> <peer> [<peer>...]\n"
    "\n"
    "Synchronize file trees across multiple peers.\n"
    "\n"
    "Running with no arguments prints this help. See the specs for full behavior.\n"
    "\n"
    "Peers:\n"
    "  /path or c:\\path                 Local path (same as file://)\n"
    "  sftp://user@host/path            Remote over SSH\n"
    "  sftp://user@host:port/path       Non-standard SSH port\n"
    "  sftp://host/path                 Remote over SSH, current OS user\n"
    "  sftp://user:password@host/path   Inline password (prefer SSH keys)\n"
    "\n"
    "Prefix modifiers:\n"
    "  +<peer>                          Canon - this peer's state wins all conflicts\n"
    "  -<peer>                          Subordinate - overwritten to match the group\n"
    "\n"
    "Fallback URLs (multiple paths to the same data):\n"
    "  [url1,url2,...]                  Try in order, first that connects wins\n"
    "  +[url1,url2,...]                 Canon peer with fallbacks\n"
    "  -[url1,url2,...]                 Subordinate peer with fallbacks\n"
    "\n"
    "Per-URL settings (query string, inside quotes):\n"
    '  "sftp://host/path?timeout-conn=60"     Connection timeout for this URL\n'
    '  "sftp://host/path?timeout-idle=10"     SFTP idle keep-alive TTL for this URL\n'
    '  "sftp://host/path?timeout-conn=60&timeout-idle=10"  Combine multiple\n'
    "\n"
    "Options:\n"
    "  --dry-run          Read-only and plan, but make no peer changes\n"
    "  --max-copies N     Max active file copies across the whole run (default: 10)\n"
    "  --retries-copy N   Give up copying after this many tries (default: 3)\n"
    "  --retries-list N   Give up listing after this many tries (default: 3)\n"
    "  --timeout-conn N   SSH handshake timeout in seconds (default: 30)\n"
    "  --timeout-idle N   SFTP idle keep-alive TTL in seconds (default: 30)\n"
    "  --verbosity LEVEL  Verbosity: error, info, debug, trace (default: info)\n"
    "  -x RELPATH         Exclude relative slash path from sync; repeatable\n"
    "  --keep-tmp-days N  Delete stale TMP staging after N days (default: 2)\n"
    "  --keep-bak-days N  Delete displaced files (BAK/) after N days (default: 90)\n"
    "  --keep-del-days N  Forget deletion records after N days (default: 180)\n"
    "\n"
    "Quick start:\n"
    "  kitchensync +c:/photos sftp://user@host/photos      First sync (c: is canon)\n"
    "  kitchensync c:/photos sftp://host/photos            Bidirectional\n"
    "  kitchensync c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate\n"
    '  kitchensync c:/photos "sftp://user:p%40ss@host/photos"  Inline password\n'
    "\n"
    "Canon (+) is required on first sync when no peer has snapshot history.\n"
    "After the first sync, bidirectional sync works without canon.\n"
    "\n"
    "Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.\n"
    "\n"
    "Displaced files are recoverable from nearby:\n"
    "  .kitchensync/BAK/ directories (kept for --keep-bak-days days).\n"
)


def run(args, timeout=15):
    return subprocess.run(
        [str(EXECUTABLE)] + args,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )


def check(failures, condition, message):
    if not condition:
        failures.append(message)


def main():
    failures = []

    # 002.1, 002.2, 002.3 — no-argument invocation
    result = run([])

    check(
        failures,
        result.returncode == 0,
        f"002.2: expected exit 0 with no arguments, got {result.returncode}",
    )
    check(
        failures,
        result.stdout.strip() == EXPECTED_HELP.strip(),
        (
            "002.1: stdout does not match expected help text verbatim.\n"
            f"  EXPECTED (stripped):\n{EXPECTED_HELP.strip()}\n"
            f"  ACTUAL (stripped):\n{result.stdout.strip()}"
        ),
    )
    check(
        failures,
        result.stderr.strip() == "",
        f"002.3: expected empty stderr with no arguments, got: {result.stderr!r}",
    )

    # 002.4 — validation error: too few peers (single peer, no valid sync group)
    result_one_peer = run(["/tmp/only-one-peer"])
    check(
        failures,
        EXPECTED_HELP.strip() in result_one_peer.stdout.strip(),
        (
            "002.4: help text not found in stdout after single-peer validation error.\n"
            f"  stdout: {result_one_peer.stdout!r}"
        ),
    )
    # The help text must appear AFTER the error message, so there must be
    # content before the help block.
    stdout_stripped = result_one_peer.stdout.strip()
    help_pos = stdout_stripped.find(EXPECTED_HELP.strip())
    check(
        failures,
        help_pos > 0,
        (
            "002.4: help text must appear after the error message, but nothing "
            f"precedes it. stdout: {result_one_peer.stdout!r}"
        ),
    )

    if failures:
        print("FAILURES:")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)

    print("All checks passed.")
    sys.exit(0)


if __name__ == "__main__":
    main()
