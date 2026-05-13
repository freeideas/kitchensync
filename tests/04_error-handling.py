#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Error-handling: unreachable peers, abort conditions, partial-failure recovery."""

from __future__ import annotations

import os, shutil, socket, sqlite3, subprocess, sys, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT) / "tmp" / "testks" / "04_error-handling"


def _run(*peer_args, timeout=60):
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT, *peer_args],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        timeout=timeout,
    )


def _bad_sftp_url(path="/nonexistent") -> str:
    """Return sftp:// URL whose port is guaranteed not listening (connection refused)."""
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    return f"sftp://localhost:{port}{path}"


def _url(path: Path) -> str:
    return path.resolve().as_uri()


def _write(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8", newline="\n")


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _snapshot_row(peer: Path, basename: str):
    db = peer / ".kitchensync" / "snapshot.db"
    with sqlite3.connect(db) as conn:
        return conn.execute(
            """
            SELECT mod_time, byte_size, last_seen, deleted_time
            FROM snapshot
            WHERE basename = ?
            """,
            (basename,),
        ).fetchone()


def _chmod_restore(path: Path, mode: int = 0o755) -> None:
    try:
        if path.exists():
            path.chmod(mode)
    except Exception:
        pass


def main() -> int:
    # Idempotent cleanup at start
    for locked in [
        TMP / "t0411" / "peer1" / "sub",
        TMP / "t0416" / "peer3",
        TMP / "t0417" / "peer3",
        TMP / "t0417b" / "peer2",
        TMP / "t0419" / "peer1" / "sub",
        TMP / "t0419" / "peer2" / "sub",
    ]:
        _chmod_restore(locked)
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)

    failures: list[str] = []

    try:
        # ── 04.7: unreachable peer skipped, run continues ─────────────────────────
        t7_p1 = TMP / "t047" / "peer1"
        t7_p2 = TMP / "t047" / "peer2"
        t7_p1.mkdir(parents=True)
        t7_p2.mkdir(parents=True)
        _write(t7_p1 / "sync.txt", "hello")

        r7 = _run(
            "-vl", "error",
            "+" + _url(t7_p1),
            _url(t7_p2),
            _bad_sftp_url("/unreachable"),
        )
        got_file7 = (t7_p2 / "sync.txt").exists()
        warned7 = "peer unreachable" in r7.stdout
        print(
            f"[04.7] exit={r7.returncode} (expect 0), "
            f"warning={warned7}, peer2 got sync.txt={got_file7}"
        )
        if r7.returncode != 0:
            failures.append(
                f"04.7: expected exit 0 (run continues past unreachable peer), got {r7.returncode}\n"
                f"  stdout: {r7.stdout!r}"
            )
        if not warned7:
            failures.append(
                "04.7: expected an error-verbosity warning for the unreachable peer\n"
                f"  stdout: {r7.stdout!r}"
            )
        if not got_file7:
            failures.append("04.7: sync.txt not copied to peer2; sync should continue with reachable peers")

        # ── 04.8: fewer than 2 reachable peers → exit 1 ──────────────────────────
        r8 = _run(_bad_sftp_url("/p1"), _bad_sftp_url("/p2"))
        print(f"[04.8] exit={r8.returncode} (expect 1)")
        if r8.returncode != 1:
            failures.append(f"04.8: expected exit 1 (fewer than 2 reachable), got {r8.returncode}")

        # ── 04.9: canon peer unreachable → exit 1 ────────────────────────────────
        t9_p1 = TMP / "t049" / "peer1"
        t9_p2 = TMP / "t049" / "peer2"
        t9_p1.mkdir(parents=True)
        t9_p2.mkdir(parents=True)
        r9 = _run("+" + _bad_sftp_url("/canon"), _url(t9_p1), _url(t9_p2))
        print(f"[04.9] exit={r9.returncode} (expect 1)")
        if r9.returncode != 1:
            failures.append(f"04.9: expected exit 1 (canon unreachable), got {r9.returncode}")

        # ── 04.10: every reachable peer is subordinate → exit 1 + specific message
        t10_p1 = TMP / "t0410" / "peer1"
        t10_p2 = TMP / "t0410" / "peer2"
        t10_p1.mkdir(parents=True)
        t10_p2.mkdir(parents=True)
        _write(t10_p1 / "file.txt", "data")
        # Establish snapshots so both peers are treated as contributing by default
        _run("+" + _url(t10_p1), _url(t10_p2))
        # Now run with both marked subordinate
        r10 = _run("-" + _url(t10_p1), "-" + _url(t10_p2))
        msg10 = "No contributing peer reachable — cannot make sync decisions"
        print(
            f"[04.10] exit={r10.returncode} (expect 1), "
            f"expected message in stdout={msg10 in r10.stdout}"
        )
        if r10.returncode != 1:
            failures.append(
                f"04.10: expected exit 1 (all subordinate), got {r10.returncode}"
            )
        if msg10 not in r10.stdout:
            failures.append(
                f"04.10: expected message {msg10!r} in stdout\n  stdout: {r10.stdout!r}"
            )

        # ── 04.11: list_dir failure on one peer excludes it from that subtree ─────
        t11_p1 = TMP / "t0411" / "peer1"
        t11_p2 = TMP / "t0411" / "peer2"
        t11_p3 = TMP / "t0411" / "peer3"
        for p in (t11_p1, t11_p2, t11_p3):
            (p / "sub").mkdir(parents=True)
        for p in (t11_p1, t11_p2, t11_p3):
            _write(p / "sub" / "shared.txt", "shared")
        # Establish snapshots across all three
        _run(
            "+" + _url(t11_p1),
            _url(t11_p2),
            _url(t11_p3),
        )
        row20_before = _snapshot_row(t11_p1, "shared.txt")
        # Add new content below peer2's sub/ — peer3 should still receive it
        # while peer1 is excluded from the failed directory and its subtree.
        _write(t11_p2 / "sub" / "newfile.txt", "new")
        (t11_p2 / "sub" / "nested").mkdir()
        _write(t11_p2 / "sub" / "nested" / "deep.txt", "deep")
        newer11 = time.time() + 5
        os.utime(t11_p2 / "sub" / "newfile.txt", (newer11, newer11))
        os.utime(t11_p2 / "sub" / "nested" / "deep.txt", (newer11, newer11))
        # Lock peer1's sub/ to trigger list_dir failure there
        (t11_p1 / "sub").chmod(0o000)
        try:
            r11 = _run(
                "-vl", "error",
                "+" + _url(t11_p1),
                _url(t11_p2),
                _url(t11_p3),
            )
        finally:
            _chmod_restore(t11_p1 / "sub")
        got_new11 = (t11_p3 / "sub" / "newfile.txt").exists()
        got_deep11 = (t11_p3 / "sub" / "nested" / "deep.txt").exists()
        excluded11 = not (t11_p1 / "sub" / "newfile.txt").exists()
        subtree_excluded11 = not (t11_p1 / "sub" / "nested" / "deep.txt").exists()
        warned11 = "listing failed" in r11.stdout and "/sub" in r11.stdout
        row20_after = _snapshot_row(t11_p1, "shared.txt")
        row20_new = _snapshot_row(t11_p1, "newfile.txt")
        row20_deep = _snapshot_row(t11_p1, "deep.txt")
        print(
            f"[04.11] peer1 excluded={excluded11}, subtree excluded={subtree_excluded11}, "
            f"warning={warned11}, peer3 got newfile={got_new11}, peer3 got deep={got_deep11}"
        )
        if not excluded11:
            failures.append(
                "04.11: peer1 received sub/newfile.txt even though its sub/ listing failed; "
                "that peer should be excluded from the affected subtree"
            )
        if not subtree_excluded11:
            failures.append(
                "04.11: peer1 received sub/nested/deep.txt even though its sub/ listing failed; "
                "that peer should be excluded from the entire affected subtree"
            )
        if not warned11:
            failures.append(
                "04.11: expected an error-verbosity listing failure for peer1 sub/\n"
                f"  stdout: {r11.stdout!r}"
            )
        if not got_new11:
            failures.append(
                "04.11: peer3/sub/newfile.txt not present; peer2 and peer3 should sync "
                "the subtree even though peer1 fails listing it\n"
                f"  stdout: {r11.stdout!r}"
            )
        if not got_deep11:
            failures.append(
                "04.11: peer3/sub/nested/deep.txt not present; unaffected peers should still "
                "sync descendants of the failed subtree\n"
                f"  stdout: {r11.stdout!r}"
            )
        print(f"[04.20] peer1 shared.txt snapshot row unchanged={row20_before == row20_after}")
        if row20_before != row20_after:
            failures.append(
                "04.20: peer1 snapshot row for sub/shared.txt changed even though "
                "peer1 failed listing that subtree"
            )
        if row20_new is not None or row20_deep is not None:
            failures.append(
                "04.20: peer1 gained snapshot rows for entries under sub/ even though "
                f"peer1 failed listing that subtree (newfile={row20_new}, deep={row20_deep})"
            )

        # ── 04.12: transfer failure logs error, skips that file, other transfers continue
        t12_p1 = TMP / "t0412" / "peer1"
        t12_p2 = TMP / "t0412" / "peer2"
        t12_p3 = TMP / "t0412" / "peer3"
        for p in (t12_p1, t12_p2, t12_p3):
            p.mkdir(parents=True)
        _write(t12_p1 / "fail.txt", "copy-me")
        (t12_p2 / ".kitchensync").mkdir()
        _write(t12_p2 / ".kitchensync" / "TMP", "blocker")
        r12 = _run("-vl", "error", "+" + _url(t12_p1), _url(t12_p2), _url(t12_p3))
        skipped12 = not (t12_p2 / "fail.txt").exists()
        continued12 = (t12_p3 / "fail.txt").exists()
        logged12 = "transfer failed for fail.txt" in r12.stdout
        print(
            f"[04.12] logged={logged12}, peer2 skipped fail.txt={skipped12}, "
            f"peer3 received fail.txt={continued12}, exit={r12.returncode}"
        )
        if r12.returncode != 0:
            failures.append(
                f"04.12: expected exit 0 after transfer failure was logged and skipped, "
                f"got {r12.returncode}\n  stdout: {r12.stdout!r}"
            )
        if not logged12:
            failures.append(
                "04.12: expected transfer failure logged at error verbosity\n"
                f"  stdout: {r12.stdout!r}"
            )
        if not skipped12:
            failures.append("04.12: peer2 received fail.txt despite transfer staging failure")
        if not continued12:
            failures.append("04.12: peer3 did not receive fail.txt; other transfers should continue")

        # ── 04.13: displacement failure → file stays in place ────────────────────
        t13_p1 = TMP / "t0413" / "peer1"
        t13_p2 = TMP / "t0413" / "peer2"
        t13_p1.mkdir(parents=True)
        t13_p2.mkdir(parents=True)
        _write(t13_p1 / "shared.txt", "data")
        # First sync: establish snapshots; peer2 gets shared.txt
        _run("+" + _url(t13_p1), _url(t13_p2))
        # peer2 gains an extra file not known to canon
        _write(t13_p2 / "extra.txt", "extra")
        # Block displacement: make .kitchensync/BAK a regular file so mkdir fails
        bak13 = t13_p2 / ".kitchensync" / "BAK"
        if bak13.is_dir():
            shutil.rmtree(bak13)
        _write(bak13, "blocker")
        try:
            r13 = _run("-vl", "error", "+" + _url(t13_p1), _url(t13_p2))
        finally:
            if bak13.is_file():
                bak13.unlink()
        still_there13 = (t13_p2 / "extra.txt").exists()
        logged13 = "displacement failed for extra.txt" in r13.stdout
        print(
            f"[04.13] logged={logged13}, extra.txt still present on peer2={still_there13}, "
            f"exit={r13.returncode}"
        )
        if r13.returncode != 0:
            failures.append(
                f"04.13: expected exit 0 after displacement failure was logged and skipped, "
                f"got {r13.returncode}\n  stdout: {r13.stdout!r}"
            )
        if not logged13:
            failures.append(
                "04.13: expected displacement failure logged at error verbosity\n"
                f"  stdout: {r13.stdout!r}"
            )
        if not still_there13:
            failures.append(
                "04.13: extra.txt was removed despite BAK/ being blocked; "
                "displacement failure should leave file in place"
            )

        # 04.14 is not reasonably testable through the CLI: it requires a transport
        # that accepts the copy and final rename but rejects only set_mod_time.

        # ── 04.15: displacement failure during copy → copy skipped, TMP cleaned ──
        t15_p1 = TMP / "t0415" / "peer1"
        t15_p2 = TMP / "t0415" / "peer2"
        (t15_p1 / "dir").mkdir(parents=True)
        (t15_p2 / "dir").mkdir(parents=True)
        _write(t15_p1 / "dir" / "file.txt", "v1")
        _write(t15_p2 / "dir" / "file.txt", "v1")
        # First sync: establish snapshots with identical content
        _run("+" + _url(t15_p1), _url(t15_p2))
        # Canon gets a newer version
        _write(t15_p1 / "dir" / "file.txt", "v2")
        newer15 = time.time() + 10
        os.utime(t15_p1 / "dir" / "file.txt", (newer15, newer15))
        # Block displacement on peer2 (existing file.txt must be displaced before copy lands)
        (t15_p2 / "dir" / ".kitchensync").mkdir()
        bak15 = t15_p2 / "dir" / ".kitchensync" / "BAK"
        if bak15.is_dir():
            shutil.rmtree(bak15)
        _write(bak15, "blocker")
        try:
            r15 = _run("-vl", "error", "+" + _url(t15_p1), _url(t15_p2))
        finally:
            if bak15.is_file():
                bak15.unlink()
        file15 = t15_p2 / "dir" / "file.txt"
        content15 = _read(file15) if file15.exists() else None
        # TMP should have no non-snapshot staged files (staging cleaned up on failure)
        tmp15 = t15_p2 / "dir" / ".kitchensync" / "TMP"
        staged15 = []
        if tmp15.exists():
            staged15 = [
                p for p in tmp15.rglob("*")
                if p.is_file() and "snapshot" not in p.name.lower()
            ]
        logged15 = "displacement failed for dir/file.txt" in r15.stdout
        print(
            f"[04.15] peer2 file.txt={content15!r} (expect 'v1'), "
            f"orphaned TMP staged files={len(staged15)}, logged={logged15}, exit={r15.returncode}"
        )
        if r15.returncode != 0:
            failures.append(
                f"04.15: expected exit 0 after displacement failure during copy was skipped, "
                f"got {r15.returncode}\n  stdout: {r15.stdout!r}"
            )
        if not logged15:
            failures.append(
                "04.15: expected displacement failure during copy to be logged\n"
                f"  stdout: {r15.stdout!r}"
            )
        if content15 != "v1":
            failures.append(
                f"04.15: expected peer2/dir/file.txt='v1' (copy skipped), got {content15!r}"
            )
        if staged15:
            failures.append(
                f"04.15: TMP staging file not removed after displacement failure: {staged15}"
            )

        # ── 04.16: unreachable peer's snapshot rows are not modified ─────────────
        t16_p1 = TMP / "t0416" / "peer1"
        t16_p2 = TMP / "t0416" / "peer2"
        t16_p3 = TMP / "t0416" / "peer3"
        for p in (t16_p1, t16_p2, t16_p3):
            p.mkdir(parents=True)
        _write(t16_p1 / "file.txt", "v1")
        _run("+" + _url(t16_p1), _url(t16_p2), _url(t16_p3))
        snap16 = t16_p3 / ".kitchensync" / "snapshot.db"
        snap16_before = snap16.read_bytes()
        _write(t16_p1 / "file.txt", "v2")
        newer16 = time.time() + 10
        os.utime(t16_p1 / "file.txt", (newer16, newer16))
        t16_p3.chmod(0o000)
        try:
            r16 = _run("-vl", "error", "+" + _url(t16_p1), _url(t16_p2), _url(t16_p3))
        finally:
            _chmod_restore(t16_p3)
        snap16_after = snap16.read_bytes()
        unchanged16 = snap16_before == snap16_after
        print(f"[04.16] exit={r16.returncode} (expect 0), peer3 snapshot unchanged={unchanged16}")
        if r16.returncode != 0:
            failures.append(
                f"04.16: expected exit 0 with two reachable peers remaining, got {r16.returncode}\n"
                f"  stdout: {r16.stdout!r}"
            )
        if not unchanged16:
            failures.append("04.16: unreachable peer3 snapshot.db changed during the run")

        # ── 04.17: snapshot-download failure (non-not-found) → peer excluded ─────
        t17_p1 = TMP / "t0417" / "peer1"
        t17_p2 = TMP / "t0417" / "peer2"
        t17_p3 = TMP / "t0417" / "peer3"
        for p in (t17_p1, t17_p2, t17_p3):
            p.mkdir(parents=True)
        _write(t17_p1 / "file.txt", "data")
        # Establish snapshots on all three
        _run(
            "+" + _url(t17_p1),
            _url(t17_p2),
            _url(t17_p3),
        )
        # Make peer3's existing snapshot unreadable so startup snapshot download
        # fails before the directory walk begins.
        snap17 = t17_p3 / ".kitchensync" / "snapshot.db"
        snap17.chmod(0o000)
        try:
            r17 = _run(
                "-vl", "error",
                "+" + _url(t17_p1),
                _url(t17_p2),
                _url(t17_p3),
            )
        finally:
            _chmod_restore(snap17, 0o644)
        # peer1 (canon) and peer2 still reachable → run should continue (exit 0)
        warned17 = "snapshot download failed" in r17.stdout or "peer unreachable" in r17.stdout
        print(
            f"[04.17] exit={r17.returncode} (expect 0), warning={warned17}, "
            f"peer1+peer2 still reachable)"
        )
        if r17.returncode != 0:
            failures.append(
                f"04.17: expected exit 0 (canon+one peer still reachable after peer3 excluded), "
                f"got {r17.returncode}\n  stdout: {r17.stdout!r}"
            )
        if not warned17:
            failures.append(
                "04.17: expected a warning when peer3 snapshot download failed\n"
                f"  stdout: {r17.stdout!r}"
            )

        t17b_p1 = TMP / "t0417b" / "peer1"
        t17b_p2 = TMP / "t0417b" / "peer2"
        for p in (t17b_p1, t17b_p2):
            p.mkdir(parents=True)
        _write(t17b_p1 / "file.txt", "data")
        _run("+" + _url(t17b_p1), _url(t17b_p2))
        snap17b = t17b_p2 / ".kitchensync" / "snapshot.db"
        snap17b.chmod(0o000)
        try:
            r17b = _run("-vl", "error", "+" + _url(t17b_p1), _url(t17b_p2))
        finally:
            _chmod_restore(snap17b, 0o644)
        print(f"[04.17] post-download reachable-count exit={r17b.returncode} (expect 1)")
        if r17b.returncode != 1:
            failures.append(
                "04.17: expected exit 1 when snapshot-download failure leaves fewer than "
                f"two reachable peers, got {r17b.returncode}\n  stdout: {r17b.stdout!r}"
            )

        t17c_p1 = TMP / "t0417c" / "peer1"
        t17c_p2 = TMP / "t0417c" / "peer2"
        t17c_p3 = TMP / "t0417c" / "peer3"
        for p in (t17c_p1, t17c_p2, t17c_p3):
            p.mkdir(parents=True)
        _write(t17c_p1 / "file.txt", "data")
        _run("+" + _url(t17c_p1), _url(t17c_p2), _url(t17c_p3))
        snap17c = t17c_p1 / ".kitchensync" / "snapshot.db"
        snap17c.chmod(0o000)
        try:
            r17c = _run("-vl", "error", "+" + _url(t17c_p1), _url(t17c_p2), _url(t17c_p3))
        finally:
            _chmod_restore(snap17c, 0o644)
        warned17c = "snapshot download failed" in r17c.stdout or "peer unreachable" in r17c.stdout
        print(
            f"[04.17] post-download canon-reachability exit={r17c.returncode} "
            f"(expect 1), warning={warned17c}"
        )
        if r17c.returncode != 1:
            failures.append(
                "04.17: expected exit 1 when snapshot-download failure excludes the canon "
                f"peer even though two non-canon peers remain, got {r17c.returncode}\n"
                f"  stdout: {r17c.stdout!r}"
            )
        if not warned17c:
            failures.append(
                "04.17: expected a warning when the canon peer snapshot download failed\n"
                f"  stdout: {r17c.stdout!r}"
            )

        # ── 04.18: snapshot-upload failure → run completes, existing snapshot untouched
        t18_p1 = TMP / "t0418" / "peer1"
        t18_p2 = TMP / "t0418" / "peer2"
        t18_p1.mkdir(parents=True)
        t18_p2.mkdir(parents=True)
        _write(t18_p1 / "file.txt", "data")
        # Establish snapshots
        _run("+" + _url(t18_p1), _url(t18_p2))
        snap18 = t18_p2 / ".kitchensync" / "snapshot.db"
        snap18_before = snap18.read_bytes() if snap18.exists() else b""
        # Block TMP staging on peer2 by replacing the TMP directory with a file
        tmp18 = t18_p2 / ".kitchensync" / "TMP"
        if tmp18.exists():
            shutil.rmtree(tmp18)
        _write(tmp18, "blocker")
        try:
            # Second sync: no file changes needed, only snapshot upload
            r18 = _run("-vl", "error", "+" + _url(t18_p1), _url(t18_p2))
        finally:
            if tmp18.is_file():
                tmp18.unlink()
        snap18_after = snap18.read_bytes() if snap18.exists() else b""
        logged18 = "snapshot upload failed" in r18.stdout
        print(
            f"[04.18] exit={r18.returncode} (expect 0), logged={logged18}, "
            f"peer2 snapshot.db unchanged={snap18_before == snap18_after}"
        )
        if r18.returncode != 0:
            failures.append(
                f"04.18: expected exit 0 (run completes despite upload failure), "
                f"got {r18.returncode}\n  stdout: {r18.stdout!r}"
            )
        if not logged18:
            failures.append(
                "04.18: expected snapshot upload failure logged at error verbosity\n"
                f"  stdout: {r18.stdout!r}"
            )
        if snap18_before != snap18_after:
            failures.append(
                "04.18: peer2 snapshot.db changed despite upload failure; "
                "existing snapshot should be left untouched"
            )
        # 04.18's retained staging-file clause is not reasonably testable through
        # the CLI: it requires a transport that accepts writeAll(tmp) but rejects
        # only the following atomic rename to snapshot.db.

        # ── 04.19: all contributing peers fail listing a dir → subtree skipped ───
        t19_p1 = TMP / "t0419" / "peer1"
        t19_p2 = TMP / "t0419" / "peer2"
        t19_p3 = TMP / "t0419" / "peer3"
        for p in (t19_p1, t19_p2, t19_p3):
            (p / "sub").mkdir(parents=True)
        for p in (t19_p1, t19_p2, t19_p3):
            _write(p / "sub" / "shared.txt", "shared")
        # Establish snapshots; peer3 is subordinate
        _run(
            "+" + _url(t19_p1),
            _url(t19_p2),
            "-" + _url(t19_p3),
        )
        # peer3 (subordinate) gains an extra file in sub/ that would normally be displaced
        _write(t19_p3 / "sub" / "extra.txt", "should-stay")
        # Lock sub/ on both contributing peers → all contributing peers fail listing sub/
        (t19_p1 / "sub").chmod(0o000)
        (t19_p2 / "sub").chmod(0o000)
        try:
            r19 = _run(
                "-vl", "error",
                "+" + _url(t19_p1),
                _url(t19_p2),
                "-" + _url(t19_p3),
            )
        finally:
            _chmod_restore(t19_p1 / "sub")
            _chmod_restore(t19_p2 / "sub")
        extra_still19 = (t19_p3 / "sub" / "extra.txt").exists()
        print(
            f"[04.19] peer3/sub/extra.txt still present={extra_still19}, exit={r19.returncode} "
            f"(subtree skipped when all contributing peers fail listing)"
        )
        if r19.returncode != 0:
            failures.append(
                f"04.19: expected exit 0 when the failed subtree is skipped, got {r19.returncode}\n"
                f"  stdout: {r19.stdout!r}"
            )
        if not extra_still19:
            failures.append(
                "04.19: peer3/sub/extra.txt was displaced even though all contributing peers "
                "failed listing sub/; subtree should be skipped entirely with no displacement"
            )

    finally:
        for locked in [
            TMP / "t0411" / "peer1" / "sub",
            TMP / "t0416" / "peer3",
            TMP / "t0417" / "peer3",
            TMP / "t0417b" / "peer2",
            TMP / "t0419" / "peer1" / "sub",
            TMP / "t0419" / "peer2" / "sub",
        ]:
            _chmod_restore(locked)
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
