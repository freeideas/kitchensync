#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end verification for reqs/003_peer-addressing.md."""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path


if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent
PROJECT_DIR = WORKSPACE_ROOT / "proj"
RELEASED_EXE = WORKSPACE_ROOT / ("released/kitchensync.exe" if sys.platform == "win32" else "released/kitchensync")


def run_kitchensync(args: list[str], *, cwd: Path, timeout_seconds: float = 8.0) -> subprocess.CompletedProcess[str] | None:
    try:
        return subprocess.run(
            [str(RELEASED_EXE), *args],
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
            check=False,
        )
    except FileNotFoundError:
        return None
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(
            args=[str(RELEASED_EXE), *args],
            returncode=124,
            stdout="",
            stderr=f"command timed out after {timeout_seconds:.1f}s",
        )


def check_case(
    failures: list[str],
    req_id: str,
    args: list[str],
    *,
    cwd: Path,
    expected_exit: int = 0,
    must_contain: tuple[str, ...] = (),
) -> subprocess.CompletedProcess[str] | None:
    result = run_kitchensync(args, cwd=cwd)
    if result is None:
        failures.append(f"{req_id}: executable not found at {RELEASED_EXE}")
        return None

    if result.returncode != expected_exit:
        failures.append(
            f"{req_id}: expected exit {expected_exit}, got {result.returncode}; "
            f"command={args!r}; stdout={result.stdout!r}; stderr={result.stderr!r}"
        )

    output = result.stdout + result.stderr
    for token in must_contain:
        if token not in output:
            failures.append(
                f"{req_id}: expected output token {token!r}; "
                f"command={args!r}; stdout={result.stdout!r}; stderr={result.stderr!r}"
            )

    return result


def make_peer(root: Path, name: str) -> Path:
    path = root / name
    path.mkdir(parents=True, exist_ok=True)
    return path


def peer_group(*items: str) -> str:
    return f"[{','.join(items)}]"


def make_file_url(path: Path) -> str:
    return path.resolve().as_uri()


def main() -> int:
    failures: list[str] = []

    if not WORKSPACE_ROOT.is_dir():
        failures.append(f"precondition: missing workspace root at {WORKSPACE_ROOT}")
    if not PROJECT_DIR.is_dir():
        failures.append(f"precondition: missing project directory at {PROJECT_DIR}")
    if not RELEASED_EXE.is_file():
        failures.append(f"precondition: missing released executable at {RELEASED_EXE}")

    if failures:
        print("FAIL: tests/test_003_peer_addressing.py (precondition)")
        for entry in failures:
            print(f"  - {entry}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
        root = Path(raw_root)
        canon = make_peer(root, "canon")
        peer = make_peer(root, "peer")

        # 003.1, 003.2, 003.6, 003.17, 003.18
        # Local bare paths are accepted, no-scheme is file-local, relative path peers work,
        # and '+' marks an unbracketed canon peer while plain peer args are normal peers.
        check_case(
            failures,
            "003.1/003.2/003.6/003.17/003.18",
            ["--dry-run", f"+{canon.name}", peer.name],
            cwd=root,
            expected_exit=0,
        )

        # 003.3
        # file:// peers parse as local peers.
        check_case(
            failures,
            "003.3",
            ["--dry-run", f"+{make_file_url(canon)}", make_file_url(peer)],
            cwd=root,
            expected_exit=0,
        )

    # 003.4
    # Absolute unix-style paths are accepted as local peer addresses on non-Windows platforms.
    if sys.platform != "win32":
        with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
            root = Path(raw_root)
            canon = make_peer(root, "unix-canon").resolve()
            peer = make_peer(root, "unix-peer").resolve()
            check_case(
                failures,
                "003.4",
                ["--dry-run", f"+{str(canon)}", str(peer)],
                cwd=root,
                expected_exit=0,
            )

    # 003.5
    # Windows drive-letter paths are accepted as local peer addresses on Windows.
    if sys.platform == "win32":
        with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
            root = Path(raw_root)
            canon = make_peer(root, "drive-canon").resolve()
            peer = make_peer(root, "drive-peer").resolve()
            check_case(
                failures,
                "003.5",
                ["--dry-run", f"+{str(canon)}", str(peer)],
                cwd=root,
                expected_exit=0,
            )

    # 003.7, 003.8, 003.9, 003.10, 003.11, 003.12, 003.13
    # SFTP URLs are accepted for parse and validation in dry-run mode without local data transfer.
    with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
        root = Path(raw_root)
        canon = make_peer(root, "canon")
        peer = make_peer(root, "peer")
        urls = {
            "003.7": "sftp://alice@127.0.0.1/photos",
            "003.8": "sftp://alice@127.0.0.1/photos",
            "003.9": "sftp://alice@127.0.0.1:2222/photos",
            "003.10": "sftp://127.0.0.1/photos",
            "003.11": "sftp://alice:secret%40word@127.0.0.1/photos",
            "003.12": "sftp://alice:p%40ss%3Aword@127.0.0.1/photos",
            "003.13": "sftp://alice@127.0.0.1//absolute//remote//path",
        }
        for req_id, url in urls.items():
            check_case(
                failures,
                req_id,
                ["--dry-run", "--timeout-conn", "1", f"+{canon.name}", peer.name, url],
                cwd=root,
                expected_exit=0,
            )

    # 003.14, 003.15, 003.20
    # Bracketed fallback groups are parsed from local path peers and '+[url...]' marks the
    # fallback peer itself as canonical.
    with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
        root = Path(raw_root)
        a = make_peer(root, "fallback-a")
        b = make_peer(root, "fallback-b")
        sink = make_peer(root, "sink")
        # 003.14, 003.20
        # Explicit '+' on the bracketed group.
        check_case(
            failures,
            "003.14/003.20",
            ["--dry-run", f"+{peer_group(a.name, b.name)}", sink.name],
            cwd=root,
            expected_exit=0,
        )

    # 003.16
    # Windows drive paths inside fallback groups are treated as local file peers.
    if sys.platform == "win32":
        with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
            root = Path(raw_root)
            a = make_peer(root, "fallback-drive-a").resolve()
            b = make_peer(root, "fallback-drive-b").resolve()
            sink = make_peer(root, "sink")
            check_case(
                failures,
                "003.16",
                ["--dry-run", f"+{peer_group(str(a), str(b))}", sink.name],
                cwd=root,
                expected_exit=0,
            )

    # 003.19, 003.21, 003.24
    # '-' prefixes mark subordinates; both unbracketed and bracketed forms are supported,
    # and multiple unbracketed subordinates are valid.
    with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
        root = Path(raw_root)
        canon = make_peer(root, "canon")
        target = make_peer(root, "target")
        one = make_peer(root, "one")
        two = make_peer(root, "two")
        fg_a = make_peer(root, "fg-a")
        fg_b = make_peer(root, "fg-b")

        check_case(
            failures,
            "003.19",
            ["--dry-run", f"+{canon.name}", target.name, f"-{one.name}"],
            cwd=root,
            expected_exit=0,
        )
        check_case(
            failures,
            "003.21",
            ["--dry-run", f"+{canon.name}", target.name, f"-{peer_group(fg_a.name, fg_b.name)}"],
            cwd=root,
            expected_exit=0,
        )
        check_case(
            failures,
            "003.24",
            ["--dry-run", f"+{canon.name}", target.name, f"-{one.name}", f"-{two.name}"],
            cwd=root,
            expected_exit=0,
        )

    # 003.22
    # '+'/'-' prefixes are rejected on members inside bracketed groups.
    with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
        root = Path(raw_root)
        canon = make_peer(root, "canon")
        a = make_peer(root, "a")
        b = make_peer(root, "b")
        check_case(
            failures,
            "003.22",
            ["--dry-run", f"+{canon.name}", peer_group(f"+{a.name}", b.name)],
            cwd=root,
            expected_exit=1,
        )

        # 003.22
        check_case(
            failures,
            "003.22",
            ["--dry-run", f"+{canon.name}", peer_group(f"-{a.name}", b.name)],
            cwd=root,
            expected_exit=1,
        )

    # 003.23
    # Only one canon '+' is accepted in one run.
    with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
        root = Path(raw_root)
        a = make_peer(root, "a")
        b = make_peer(root, "b")
        c = make_peer(root, "c")
        check_case(
            failures,
            "003.23",
            ["--dry-run", f"+{a.name}", f"+{b.name}", c.name],
            cwd=root,
            expected_exit=1,
        )

    # 003.25, 003.26
    # SFTP per-URL timeout settings are accepted in dry-run.
    with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
        root = Path(raw_root)
        canon = make_peer(root, "canon")
        peer = make_peer(root, "peer")
        check_case(
            failures,
            "003.25",
            [
                "--dry-run",
                "--timeout-conn",
                "1",
                f"+{canon.name}",
                peer.name,
                "sftp://alice@127.0.0.1/photos?timeout-conn=12",
            ],
            cwd=root,
            expected_exit=0,
        )
        check_case(
            failures,
            "003.26",
            [
                "--dry-run",
                "--timeout-conn",
                "1",
                f"+{canon.name}",
                peer.name,
                "sftp://alice@127.0.0.1/photos?timeout-idle=15",
            ],
            cwd=root,
            expected_exit=0,
        )

    # 003.27
    # not reasonably testable: 003.27 (per-URL query settings are scoped to individual URLs inside fallback groups)

    # 003.28
    # Unsupported per-URL keys are rejected during argument validation.
    with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
        root = Path(raw_root)
        canon = make_peer(root, "canon")
        peer = make_peer(root, "peer")
        check_case(
            failures,
            "003.28",
            [
                "--dry-run",
                "--timeout-conn",
                "1",
                f"+{canon.name}",
                peer.name,
                "sftp://alice@127.0.0.1/photos?unsupported=value",
            ],
            cwd=root,
            expected_exit=1,
        )

    # 003.29
    # Scheme lowercase normalization is exercised by accepting uppercase scheme input.
    with tempfile.TemporaryDirectory(prefix="ks_003_") as raw_root:
        root = Path(raw_root)
        canon = make_peer(root, "canon")
        peer = make_peer(root, "peer")
        check_case(
            failures,
            "003.29",
            [
                "--dry-run",
                "--timeout-conn",
                "1",
                f"+{canon.name}",
                peer.name,
                "SFTP://alice@127.0.0.1/photos",
            ],
            cwd=root,
            expected_exit=0,
        )

    # 003.30
    # not reasonably testable: 003.30 (hostname lowercasing before peer identity compare)
    # 003.31
    # not reasonably testable: 003.31 (default SFTP port removal before peer comparison)
    # 003.32
    # not reasonably testable: 003.32 (path slash-collapsing normalization is internal)
    # 003.33
    # not reasonably testable: 003.33 (trailing slash removal is internal)
    # 003.34
    # not reasonably testable: 003.34 (bare paths normalized to file:// before compare)
    # 003.35
    # not reasonably testable: 003.35 (file:// absolute-path normalization)
    # 003.36
    # not reasonably testable: 003.36 (percent-decodes are internal to peer identity comparison)
    # 003.37
    # not reasonably testable: 003.37 (query stripping before comparison)
    # 003.38
    # not reasonably testable: 003.38 (current-OS-user insertion into username-omitted sftp URLs)

    if failures:
        print("FAIL: test_003_peer_addressing.py")
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("PASS: test_003_peer_addressing.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
