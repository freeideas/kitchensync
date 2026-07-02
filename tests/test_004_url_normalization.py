# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
RELEASED_EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")


def workspace_root() -> Path:
    if WORKSPACE_ROOT.exists():
        return WORKSPACE_ROOT
    return Path(__file__).resolve().parents[1]


def released_exe() -> Path:
    if RELEASED_EXE.exists():
        return RELEASED_EXE
    return workspace_root() / "released" / "kitchensync.exe"


def file_url(path: Path) -> str:
    return path.resolve().as_uri()


def child_file_url(parent: Path, child_url_segment: str) -> str:
    return file_url(parent).rstrip("/") + "/" + child_url_segment


def run_kitchensync(args: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(released_exe()), *args],
        cwd=str(cwd),
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=20,
        check=False,
    )


def expect_same_identity(
    failures: list[str],
    label: str,
    peer_a: str,
    peer_b: str,
    cwd: Path,
) -> None:
    result = run_kitchensync(["--dry-run", f"+{peer_a}", peer_b], cwd)
    if result.stderr != "":
        failures.append(f"{label}: stderr should be empty, got {result.stderr!r}")
    if result.returncode == 0:
        failures.append(
            f"{label}: normalized duplicate peers should not form two distinct "
            f"reachable peers; stdout was {result.stdout!r}"
        )


def expect_distinct_identity(
    failures: list[str],
    label: str,
    peer_a: str,
    peer_b: str,
    cwd: Path,
) -> None:
    result = run_kitchensync(["--dry-run", f"+{peer_a}", peer_b], cwd)
    if result.stderr != "":
        failures.append(f"{label}: stderr should be empty, got {result.stderr!r}")
    if result.returncode != 0:
        failures.append(
            f"{label}: distinct normalized peers should be accepted; "
            f"exit {result.returncode}, stdout {result.stdout!r}"
        )
    if "dry run" not in result.stdout.lower():
        failures.append(f"{label}: dry-run stdout should mention dry run")


def make_dir(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    return path


def check_local_file_url_normalization(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-url-norm-") as tmp_text:
        tmp = Path(tmp_text)

        bare = make_dir(tmp / "bare")
        expect_same_identity(
            failures,
            "004.1 bare local path becomes file URL",
            str(bare),
            file_url(bare),
            tmp,
        )

        relative = make_dir(tmp / "relative")
        expect_same_identity(
            failures,
            "004.3 relative path resolves from cwd before identity comparison",
            "relative",
            file_url(relative),
            tmp,
        )

        scheme = make_dir(tmp / "scheme")
        expect_same_identity(
            failures,
            "004.4 file URL scheme comparison is case-insensitive",
            file_url(scheme).replace("file://", "FILE://", 1),
            file_url(scheme),
            tmp,
        )

        slash = make_dir(tmp / "slash" / "leaf")
        expect_same_identity(
            failures,
            "004.8 consecutive path slashes collapse before identity comparison",
            child_file_url(tmp / "slash", "/leaf"),
            file_url(slash),
            tmp,
        )

        trailing = make_dir(tmp / "trailing")
        expect_same_identity(
            failures,
            "004.9 trailing path slash is removed before identity comparison",
            file_url(trailing) + "/",
            file_url(trailing),
            tmp,
        )

        unreserved = make_dir(tmp / "tilde~name")
        expect_same_identity(
            failures,
            "004.10 percent-encoded unreserved path characters are decoded",
            child_file_url(tmp, "tilde%7Ename"),
            file_url(unreserved),
            tmp,
        )

        encoded_reserved = make_dir(tmp / "encoded%2Fslash")
        decoded_reserved = make_dir(tmp / "encoded" / "slash")
        expect_distinct_identity(
            failures,
            "004.11 percent-encoded reserved path characters remain encoded",
            child_file_url(tmp, "encoded%2Fslash"),
            file_url(decoded_reserved),
            tmp,
        )
        if not encoded_reserved.exists():
            failures.append("004.11 setup directory unexpectedly disappeared")

        query = make_dir(tmp / "query")
        expect_same_identity(
            failures,
            "004.12 query parameters are stripped from peer identity",
            file_url(query) + "?timeout-conn=30&timeout-idle=30",
            file_url(query),
            tmp,
        )

        if os.name == "nt":
            drive = make_dir(tmp / "drive")
            expect_same_identity(
                failures,
                "004.2 Windows drive path becomes file URL",
                str(drive),
                file_url(drive),
                tmp,
            )
        else:
            # not reasonably testable: 004.2 requires a Windows drive path.
            pass


def main() -> int:
    failures: list[str] = []

    if not released_exe().exists():
        failures.append(f"released executable does not exist: {released_exe()}")
    else:
        check_local_file_url_normalization(failures)

    # not reasonably testable: 004.5 requires observing SFTP hostname identity
    # without a product surface that prints normalized peer URLs.
    # not reasonably testable: 004.6 requires observing default SFTP port identity
    # without binding a test SFTP server to port 22.
    # not reasonably testable: 004.7 requires observing non-default SFTP port
    # identity without a normalized peer URL surface.
    # not reasonably testable: 004.13 requires observing implicit OS username
    # insertion separately from SFTP authentication.
    # not reasonably testable: 004.14 requires observing explicit SFTP username
    # retention separately from SFTP authentication.

    if failures:
        print("FAIL")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
