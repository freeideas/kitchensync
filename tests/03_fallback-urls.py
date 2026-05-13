#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises fallback URL bracket syntax: 03.52–03.57."""

from __future__ import annotations

import os, shutil, subprocess, sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "03_fallback-urls"


def run_cli(*args, timeout=60):
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT, *args],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        timeout=timeout,
    )


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)

    failures = []

    try:
        # --- 03.52: [url1,url2,...] is treated as a single peer ---
        # +[peer_b,peer_c] as the canon peer; peer_b has a file; peer_dest is empty.
        # Exit 0 and the file appearing in peer_dest proves the bracket was one peer.
        peer_b_52 = TMP / "03.52_b"
        peer_c_52 = TMP / "03.52_c"
        dest_52 = TMP / "03.52_dest"
        peer_b_52.mkdir()
        peer_c_52.mkdir()
        dest_52.mkdir()
        (peer_b_52 / "hello.txt").write_text("from_b")
        bracket_52 = f"[{peer_b_52.resolve().as_uri()},{peer_c_52.resolve().as_uri()}]"
        proc = run_cli("+" + bracket_52, dest_52.resolve().as_uri())
        print(f"[03.52] bracket peer synced as single peer: exit {proc.returncode}")
        if proc.returncode != 0:
            failures.append(
                f"03.52: expected exit 0, got {proc.returncode}; "
                f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
            )
        elif not (dest_52 / "hello.txt").exists():
            failures.append("03.52: hello.txt not synced — bracket not treated as single peer")

        # --- 03.53: URLs tried in the order given ---
        # First URL is /dev/null/... (creation fails → peer unreachable for that URL).
        # Second URL is a valid directory with a file.
        # If URLs are tried in order, the second URL succeeds and the file is synced.
        peer_fallback_53 = TMP / "03.53_fallback"
        dest_53 = TMP / "03.53_dest"
        peer_fallback_53.mkdir()
        dest_53.mkdir()
        (peer_fallback_53 / "fallback.txt").write_text("from_fallback")
        bracket_53 = f"[file:///dev/null/ks_unreachable_53,{peer_fallback_53.resolve().as_uri()}]"
        proc = run_cli("+" + bracket_53, dest_53.resolve().as_uri())
        print(f"[03.53] URLs tried in order (bad→good): exit {proc.returncode}")
        if proc.returncode != 0:
            failures.append(
                f"03.53: expected exit 0 (second URL succeeded), got {proc.returncode}; "
                f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
            )
        elif not (dest_53 / "fallback.txt").exists():
            failures.append("03.53: fallback.txt not synced — second URL was not tried")

        # --- 03.54: First URL that connects is used; remaining URLs are not tried ---
        # peer_b has file_a.txt; peer_c has file_b.txt. Both directories are reachable.
        # First URL (peer_b) connects → only file_a.txt ends up in dest; file_b.txt does not.
        peer_b_54 = TMP / "03.54_b"
        peer_c_54 = TMP / "03.54_c"
        dest_54 = TMP / "03.54_dest"
        peer_b_54.mkdir()
        peer_c_54.mkdir()
        dest_54.mkdir()
        (peer_b_54 / "file_a.txt").write_text("from_b")
        (peer_c_54 / "file_b.txt").write_text("from_c")
        bracket_54 = f"[{peer_b_54.resolve().as_uri()},{peer_c_54.resolve().as_uri()}]"
        proc = run_cli("+" + bracket_54, dest_54.resolve().as_uri())
        print(f"[03.54] first URL used, second not tried: exit {proc.returncode}")
        if proc.returncode != 0:
            failures.append(
                f"03.54: expected exit 0, got {proc.returncode}; "
                f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
            )
        else:
            if not (dest_54 / "file_a.txt").exists():
                failures.append("03.54: file_a.txt not synced — first URL was not used")
            if (dest_54 / "file_b.txt").exists():
                failures.append("03.54: file_b.txt in dest — second URL was incorrectly accessed")

        # --- 03.55: Every URL fails → peer unreachable → same handling as single failed URL ---
        # Both bracket URLs point to paths under /dev/null (creation will fail).
        # With the bracket peer unreachable and only one peer reachable, exit must be non-zero.
        dest_55 = TMP / "03.55_dest"
        dest_55.mkdir()
        (dest_55 / "canary.txt").write_text("canary")
        bracket_55 = "[file:///dev/null/ks_unreachable_55a,file:///dev/null/ks_unreachable_55b]"
        proc = run_cli("+" + dest_55.resolve().as_uri(), bracket_55)
        print(f"[03.55] all bracket URLs fail → unreachable: exit {proc.returncode}")
        if proc.returncode == 0:
            failures.append(
                "03.55: expected non-zero exit when all bracket URLs fail, got exit 0"
            )

        # --- 03.56: +/- prefix on bracket applies to the whole peer ---
        # Sub-test A: + on bracket recognized as canon on first sync.
        peer_b_56a = TMP / "03.56a_b"
        peer_c_56a = TMP / "03.56a_c"
        dest_56a = TMP / "03.56a_dest"
        peer_b_56a.mkdir()
        peer_c_56a.mkdir()
        dest_56a.mkdir()
        (peer_b_56a / "content.txt").write_text("from_b")
        bracket_56a = f"[{peer_b_56a.resolve().as_uri()},{peer_c_56a.resolve().as_uri()}]"
        proc = run_cli("+" + bracket_56a, dest_56a.resolve().as_uri())
        print(f"[03.56a] + prefix on bracket accepted as canon: exit {proc.returncode}")
        if proc.returncode != 0:
            failures.append(
                f"03.56a: +[bracket] not accepted as canon; "
                f"exit {proc.returncode} stdout={proc.stdout!r} stderr={proc.stderr!r}"
            )

        # Sub-test B: - on bracket makes the whole peer subordinate.
        # Setup: first sync with +dest + peer_b to establish snapshots (both empty).
        # Then add a file to peer_b and run +dest -[peer_b,peer_c].
        # The bracket peer is forced subordinate → its unique file is NOT copied to dest.
        peer_b_56b = TMP / "03.56b_b"
        peer_c_56b = TMP / "03.56b_c"
        dest_56b = TMP / "03.56b_dest"
        peer_b_56b.mkdir()
        peer_c_56b.mkdir()
        dest_56b.mkdir()
        url_b_56b = peer_b_56b.resolve().as_uri()
        url_c_56b = peer_c_56b.resolve().as_uri()
        url_dest_56b = dest_56b.resolve().as_uri()
        setup = run_cli("+" + url_dest_56b, url_b_56b)
        if setup.returncode == 0:
            (peer_b_56b / "subordinate_only.txt").write_text("sub_data")
            bracket_56b = f"[{url_b_56b},{url_c_56b}]"
            proc = run_cli("+" + url_dest_56b, "-" + bracket_56b)
            print(f"[03.56b] - prefix on bracket: subordinate file not in dest: exit {proc.returncode}")
            if proc.returncode != 0:
                failures.append(
                    f"03.56b: -[bracket] sync failed; "
                    f"exit {proc.returncode} stdout={proc.stdout!r} stderr={proc.stderr!r}"
                )
            elif (dest_56b / "subordinate_only.txt").exists():
                failures.append(
                    "03.56b: subordinate_only.txt copied to dest — "
                    "-[bracket] not treated as subordinate"
                )
        else:
            failures.append(
                f"03.56b: setup sync failed (cannot test -bracket); "
                f"exit {setup.returncode} stdout={setup.stdout!r} stderr={setup.stderr!r}"
            )

        # --- 03.57: Per-URL query string settings attach to individual URLs inside bracket ---
        # Append ?mc=5 to the first URL and ?mc=3 to the second; file:// ignores mc but
        # the syntax must be accepted and the path used correctly after stripping the query.
        peer_b_57 = TMP / "03.57_b"
        peer_c_57 = TMP / "03.57_c"
        dest_57 = TMP / "03.57_dest"
        peer_b_57.mkdir()
        peer_c_57.mkdir()
        dest_57.mkdir()
        (peer_b_57 / "test.txt").write_text("from_b")
        bracket_57 = (
            f"[{peer_b_57.resolve().as_uri()}?mc=5,"
            f"{peer_c_57.resolve().as_uri()}?mc=3]"
        )
        proc = run_cli("+" + bracket_57, dest_57.resolve().as_uri())
        print(f"[03.57] per-URL query strings accepted: exit {proc.returncode}")
        if proc.returncode != 0:
            failures.append(
                f"03.57: per-URL query string syntax rejected; "
                f"exit {proc.returncode} stdout={proc.stdout!r} stderr={proc.stderr!r}"
            )
        elif not (dest_57 / "test.txt").exists():
            failures.append(
                "03.57: test.txt not synced — URL path not correctly stripped of query string"
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
