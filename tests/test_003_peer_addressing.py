#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end test for reqs/003_peer-addressing.md."""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync")
RELEASED_EXE = (
    WORKSPACE_ROOT / "released" / "kitchensync.exe"
    if os.name == "nt"
    else WORKSPACE_ROOT / "released" / "kitchensync"
)


def _run_kitchensync(
    args: list[str],
    *,
    cwd: Path,
    timeout_seconds: float = 12.0,
) -> subprocess.CompletedProcess[str] | None:
    try:
        return subprocess.run(
            [str(RELEASED_EXE), *args],
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired:
        return None
    except FileNotFoundError:
        return subprocess.CompletedProcess(
            args=[str(RELEASED_EXE), *args],
            returncode=127,
            stdout="",
            stderr="released executable not found",
        )


def _assert_exit_code(
    failures: list[str],
    req_id: str,
    result: subprocess.CompletedProcess[str] | None,
    expected_exit: int,
    *,
    command: list[str],
    message: str | None = None,
) -> None:
    if result is None:
        failures.append(f"{req_id}: command timed out for {command!r}")
        return

    if result.returncode != expected_exit:
        extra = f" {message}" if message else ""
        failures.append(
            f"{req_id}: expected exit {expected_exit} for {command!r}, "
            f"got {result.returncode}.{extra} stdout={result.stdout!r} stderr={result.stderr!r}"
        )


def _assert_stderr_empty(
    failures: list[str],
    req_id: str,
    result: subprocess.CompletedProcess[str] | None,
    command: list[str],
) -> None:
    if result is None:
        return
    if result.stderr.strip():
        failures.append(
            f"{req_id}: stderr was not empty for {command!r}: {result.stderr!r}"
        )


def _seed_peer(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)
    (path / "seed.txt").write_text("seed", encoding="utf-8")


def _file_url(path: Path) -> str:
    return path.resolve().as_uri()


def _run_and_check(
    failures: list[str],
    req_id: str,
    command: list[str],
    *,
    cwd: Path,
    expect_exit: int,
    require_empty_stderr: bool = True,
) -> subprocess.CompletedProcess[str] | None:
    result = _run_kitchensync(command, cwd=cwd)
    _assert_exit_code(failures, req_id, result, expect_exit, command=command)
    if result is not None and expect_exit == 0 and require_empty_stderr:
        _assert_stderr_empty(failures, req_id, result, command)
    return result


def _fallback_group(*items: str) -> str:
    return f"[{','.join(items)}]"


def main() -> int:
    failures: list[str] = []

    if not RELEASED_EXE.is_file():
        failures.append(f"precondition: released executable not found at {RELEASED_EXE}")

    # 003.1, 003.2, 003.6, 003.17, 003.18
    # Relative bare paths are accepted local peers. A peer without +/- is normal.
    # + marks the unbracketed canon peer.
    with tempfile.TemporaryDirectory(prefix="ks_003_relative_") as tmpdir:
        root = Path(tmpdir)
        canon = root / "canon"
        peer = root / "peer"
        _seed_peer(canon)
        _seed_peer(peer)

        _run_and_check(
            failures,
            "003.1/003.2/003.6/003.17/003.18",
            ["--dry-run", f"+{canon.name}", peer.name],
            cwd=root,
        )

    # 003.3
    # Explicit file:// peers are local peers.
    with tempfile.TemporaryDirectory(prefix="ks_003_fileurl_") as tmpdir:
        root = Path(tmpdir)
        canon = root / "canon"
        peer = root / "peer"
        _seed_peer(canon)
        _seed_peer(peer)

        _run_and_check(
            failures,
            "003.3",
            ["--dry-run", f"+{_file_url(canon)}", _file_url(peer)],
            cwd=root,
        )

    # 003.4
    # Absolute Unix-style paths are accepted as local peers.
    if os.name != "nt":
        with tempfile.TemporaryDirectory(prefix="ks_003_abs_unix_") as tmpdir:
            root = Path(tmpdir)
            canon = root / "canon_abs"
            peer = root / "peer_abs"
            _seed_peer(canon)
            _seed_peer(peer)

            absolute_canon = str(canon.resolve())
            _run_and_check(
                failures,
                "003.4",
                ["--dry-run", f"+{absolute_canon}", str(peer)],
                cwd=root,
            )
    else:
        # not reasonably testable on this platform using a local-only CLI test.
        pass

    # 003.5
    # Windows drive-letter absolute paths are accepted as local peers.
    if os.name == "nt":
        with tempfile.TemporaryDirectory(prefix="ks_003_drive_") as tmpdir:
            root = Path(tmpdir)
            canon = root / "canon_drive"
            peer = root / "peer_drive"
            _seed_peer(canon)
            _seed_peer(peer)

            absolute_canon = str(canon.resolve())
            _run_and_check(
                failures,
                "003.5",
                ["--dry-run", f"+{absolute_canon}", str(peer)],
                cwd=root,
            )

    # 003.14, 003.15, 003.20
    # Bracketed peer list is one fallback peer.
    # Bare paths are accepted in brackets as file:// peers.
    # A + on the bracket marks the whole fallback peer canon.
    with tempfile.TemporaryDirectory(prefix="ks_003_fallback_") as tmpdir:
        root = Path(tmpdir)
        primary = root / "peer_primary"
        secondary = root / "peer_secondary"
        target = root / "target"
        _seed_peer(primary)
        _seed_peer(secondary)
        _seed_peer(target)

        _run_and_check(
            failures,
            "003.14/003.15/003.20",
            ["--dry-run", f"+{_fallback_group(str(primary), str(secondary))}", str(target)],
            cwd=root,
        )

    # 003.16
    # Windows drive-paths are accepted inside bracketed fallback peers.
    if os.name == "nt":
        with tempfile.TemporaryDirectory(prefix="ks_003_fallback_drive_") as tmpdir:
            root = Path(tmpdir)
            primary = root / "peer_primary"
            secondary = root / "peer_secondary"
            target = root / "target"
            _seed_peer(primary)
            _seed_peer(secondary)
            _seed_peer(target)

            _run_and_check(
                failures,
                "003.16",
                ["--dry-run", f"+{_fallback_group(str(primary), str(secondary))}", str(target)],
                cwd=root,
            )

    # 003.19
    # - before an unbracketed peer marks it subordinate.
    with tempfile.TemporaryDirectory(prefix="ks_003_subordinate_unbracketed_") as tmpdir:
        root = Path(tmpdir)
        canon = root / "canon"
        subordinate = root / "subordinate"
        _seed_peer(canon)
        _seed_peer(subordinate)

        _run_and_check(
            failures,
            "003.19",
            ["--dry-run", f"+{canon}", f"-{subordinate}"],
            cwd=root,
        )

    # 003.21
    # - before bracketed fallback URLs marks the whole fallback peer subordinate.
    with tempfile.TemporaryDirectory(prefix="ks_003_subordinate_bracket_") as tmpdir:
        root = Path(tmpdir)
        canon = root / "canon"
        first = root / "first"
        second = root / "second"
        _seed_peer(canon)
        _seed_peer(first)
        _seed_peer(second)

        _run_and_check(
            failures,
            "003.21",
            ["--dry-run", f"+{canon}", f"-{_fallback_group(str(first), str(second))}"],
            cwd=root,
        )

    # 003.22
    # + or - inside a bracketed fallback list is rejected.
    with tempfile.TemporaryDirectory(prefix="ks_003_reject_inner_prefix_") as tmpdir:
        root = Path(tmpdir)
        canon = root / "canon"
        first = root / "first"
        second = root / "second"
        _seed_peer(canon)
        _seed_peer(first)
        _seed_peer(second)

        _run_and_check(
            failures,
            "003.22",
            ["--dry-run", f"+{canon}", f"{_fallback_group(f'+{first}', str(second))}"],
            cwd=root,
            expect_exit=1,
            require_empty_stderr=False,
        )

    # 003.23
    # At most one canon prefix is accepted in a run.
    with tempfile.TemporaryDirectory(prefix="ks_003_one_canon_") as tmpdir:
        root = Path(tmpdir)
        first = root / "first"
        second = root / "second"
        third = root / "third"
        _seed_peer(first)
        _seed_peer(second)
        _seed_peer(third)

        _run_and_check(
            failures,
            "003.23",
            ["--dry-run", f"+{first}", f"+{second}", str(third)],
            cwd=root,
            expect_exit=1,
            require_empty_stderr=False,
        )

    # 003.24
    # Multiple subordinate prefixes are valid in a single run.
    with tempfile.TemporaryDirectory(prefix="ks_003_multi_subordinate_") as tmpdir:
        root = Path(tmpdir)
        canon = root / "canon"
        first = root / "first"
        second = root / "second"
        _seed_peer(canon)
        _seed_peer(first)
        _seed_peer(second)

        _run_and_check(
            failures,
            "003.24",
            ["--dry-run", f"+{canon}", f"-{first}", f"-{second}"],
            cwd=root,
        )

    # 003.7, 003.8, 003.9, 003.10, 003.11, 003.12, 003.13
    # SFTP URL syntaxes are accepted as parse-valid local command-line peers (the SFTP peers may be unreachable).
    with tempfile.TemporaryDirectory(prefix="ks_003_sftp_parse_") as tmpdir:
        root = Path(tmpdir)
        canon = root / "canon"
        peer = root / "peer"
        _seed_peer(canon)
        _seed_peer(peer)

        sftp_urls = {
            "003.7": "sftp://alice@127.0.0.1/photos",
            "003.8": "sftp://alice@127.0.0.1/photos",
            "003.9": "sftp://alice@127.0.0.1:2222/photos",
            "003.10": "sftp://127.0.0.1/photos",
            "003.11": "sftp://alice:secret%40word@127.0.0.1/photos",
            "003.12": "sftp://alice:p%40ss%3Aword@127.0.0.1/photos",
            "003.13": "sftp://alice@127.0.0.1//absolute//remote//path",
        }

        for req_id, url in sftp_urls.items():
            _run_and_check(
                failures,
                req_id,
                [
                    "--dry-run",
                    "--timeout-conn",
                    "1",
                    f"+{canon}",
                    str(peer),
                    url,
                ],
                cwd=root,
            )

    # 003.25, 003.26
    # Accepted query params are treated as per-URL settings.
    with tempfile.TemporaryDirectory(prefix="ks_003_sftp_query_accept_") as tmpdir:
        root = Path(tmpdir)
        canon = root / "canon"
        peer = root / "peer"
        _seed_peer(canon)
        _seed_peer(peer)

        _run_and_check(
            failures,
            "003.25",
            [
                "--dry-run",
                "--timeout-conn",
                "1",
                f"+{canon}",
                str(peer),
                "sftp://alice@127.0.0.1/photos?timeout-conn=5",
            ],
            cwd=root,
        )
        _run_and_check(
            failures,
            "003.26",
            [
                "--dry-run",
                "--timeout-conn",
                "1",
                f"+{canon}",
                str(peer),
                "sftp://alice@127.0.0.1/photos?timeout-idle=8",
            ],
            cwd=root,
        )

    # 003.27
    # 003.27 requires transport-level observation and is not directly observable from this CLI-only surface.
    # 003.28: unsupported per-URL query keys are rejected during argument validation.
    with tempfile.TemporaryDirectory(prefix="ks_003_sftp_query_bad_") as tmpdir:
        root = Path(tmpdir)
        canon = root / "canon"
        peer = root / "peer"
        _seed_peer(canon)
        _seed_peer(peer)

        _run_and_check(
            failures,
            "003.28",
            [
                "--dry-run",
                "--timeout-conn",
                "1",
                f"+{canon}",
                str(peer),
                "sftp://alice@127.0.0.1/photos?bad-param=5",
            ],
            cwd=root,
            expect_exit=1,
            require_empty_stderr=False,
        )

    # not reasonably testable from released executable CLI/log surface alone:
    # 003.29 -- URL scheme lower-casing before peer lookup
    # 003.30 -- URL hostname lower-casing before peer lookup
    # 003.31 -- default-port stripping before peer lookup
    # 003.32 -- consecutive slash collapsing before peer lookup
    # 003.33 -- trailing slash removal before peer lookup
    # 003.34 -- bare paths normalized to file:// before peer lookup
    # 003.35 -- file:// paths resolved to absolute paths before peer lookup
    # 003.36 -- percent-decoding in peer identity normalization
    # 003.37 -- query stripping in identity normalization
    # 003.38 -- default OS username insertion in SFTP identity normalization

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        print(f"FAILED: {len(failures)} checks failed.")
        return 1

    print("PASS: tests/test_003_peer_addressing.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


