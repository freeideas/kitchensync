#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""URL normalization: canonical form before any comparison or lookup."""

from __future__ import annotations

import getpass, os, shutil, subprocess, sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "02_url-normalization"
PEER1 = TMP / "peer1"
PEER2 = TMP / "peer2"
SFTP_DIR = TMP / "sftp_peer"

FIRST_SYNC_MSG = "First sync? Mark the authoritative peer with a leading +"


def invoke(args, timeout=30):
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT] + args,
        capture_output=True, text=True, encoding="utf-8", timeout=timeout,
    )


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)
    PEER1.mkdir(parents=True)
    PEER2.mkdir(parents=True)
    SFTP_DIR.mkdir(parents=True)

    failures = []
    user = getpass.getuser()
    peer1_abs = str(PEER1.resolve())
    peer1_url = PEER1.resolve().as_uri()
    peer2_url = PEER2.resolve().as_uri()
    sftp_abs = str(SFTP_DIR.resolve())

    try:
        # 02.12 — bare path normalized to file:// URL
        print("[02.12] bare absolute path accepted as file:// URL")
        proc = invoke([peer1_abs, peer2_url])
        combined = proc.stdout + proc.stderr
        if proc.returncode != 1 or FIRST_SYNC_MSG not in combined:
            failures.append(
                f"02.12: expected exit 1 + first-sync guidance for bare path, "
                f"got rc={proc.returncode}\n  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )

        # 02.13 — scheme and hostname of SFTP URL normalized to lowercase
        # Two URLs identical except for case → same peer after normalization → dedup → < 2 peers → exit 1
        print("[02.13] SFTP scheme/hostname case-normalized → same peer → dedup → exit 1")
        sftp_upper = f"SFTP://{user}@LOCALHOST{sftp_abs}"
        sftp_lower = f"sftp://{user}@localhost{sftp_abs}"
        proc = invoke([sftp_upper, sftp_lower])
        if proc.returncode != 1:
            failures.append(
                f"02.13: expected exit 1 (peers differ only by case), "
                f"got rc={proc.returncode}\n  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )

        # 02.14 — default port :22 on sftp:// URL removed during normalization
        print("[02.14] sftp:22 and sftp without port → same peer → dedup → exit 1")
        sftp_port22 = f"sftp://{user}@localhost:22{sftp_abs}"
        sftp_no_port = f"sftp://{user}@localhost{sftp_abs}"
        proc = invoke([sftp_port22, sftp_no_port])
        if proc.returncode != 1:
            failures.append(
                f"02.14: expected exit 1 (peers differ only by :22), "
                f"got rc={proc.returncode}\n  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )

        # 02.15 — consecutive slashes in path collapsed
        # Insert an extra / before "peer1" → //peer1 → collapses to /peer1 → same directory
        print("[02.15] consecutive slashes in file path collapsed → resolves to peer dir → first-sync")
        double_slash_url = peer1_url[:-6] + "//peer1"  # peer1_url ends with "/peer1" (6 chars)
        proc = invoke([double_slash_url, peer2_url])
        combined = proc.stdout + proc.stderr
        if proc.returncode != 1 or FIRST_SYNC_MSG not in combined:
            failures.append(
                f"02.15: expected exit 1 + first-sync guidance for URL with '//', "
                f"got rc={proc.returncode}\n  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )

        # 02.16 — query-string parameters stripped from URL identity
        # file:///path and file:///path?mc=5 → same peer after stripping → dedup → exit 1
        print("[02.16] query-string stripped from URL identity → same peer → dedup → exit 1")
        url_qs = peer1_url + "?mc=5"
        proc = invoke([url_qs, peer1_url])
        if proc.returncode != 1:
            failures.append(
                f"02.16: expected exit 1 (peers differ only by ?mc=5), "
                f"got rc={proc.returncode}\n  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )

        # 02.17 — SFTP URL with no username gets current OS user inserted
        print("[02.17] SFTP no-username URL gains OS user → same as explicit-user URL → dedup → exit 1")
        sftp_no_user = f"sftp://localhost{sftp_abs}"
        sftp_with_user = f"sftp://{user}@localhost{sftp_abs}"
        proc = invoke([sftp_no_user, sftp_with_user])
        if proc.returncode != 1:
            failures.append(
                f"02.17: expected exit 1 (peers differ only by absent username), "
                f"got rc={proc.returncode}\n  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )

        # 02.32 — trailing slash on path removed during normalization
        # file:///path/ and file:///path → same peer after stripping → dedup → exit 1
        print("[02.32] trailing slash stripped → same peer → dedup → exit 1")
        url_trailing = peer1_url + "/"
        proc = invoke([url_trailing, peer1_url])
        if proc.returncode != 1:
            failures.append(
                f"02.32: expected exit 1 (peers differ only by trailing /), "
                f"got rc={proc.returncode}\n  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )

        # 02.33 — percent-encoded unreserved characters decoded
        # Encode 'p' (%70) in "peer1" → /%70eer1 → decoded → /peer1 → same directory → first-sync
        print("[02.33] percent-encoded unreserved char in path decoded → resolves to peer dir → first-sync")
        url_encoded = peer1_url[:-6] + "/%70eer1"  # replaces "/peer1" with "/%70eer1"
        proc = invoke([url_encoded, peer2_url])
        combined = proc.stdout + proc.stderr
        if proc.returncode != 1 or FIRST_SYNC_MSG not in combined:
            failures.append(
                f"02.33: expected exit 1 + first-sync guidance for percent-encoded path, "
                f"got rc={proc.returncode}\n  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}"
            )

    finally:
        shutil.rmtree(TMP, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
