#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end verification for reqs/011_displacement-and-staging-cleanup.md."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Callable
from urllib.parse import quote

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE_ROOT = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync")
PROJECT_DIR = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\proj")
WINDOWS_EXE_PATH = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\released\\kitchensync.exe")
POSIX_EXE_PATH = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\released\\kitchensync")
RELEASED_EXE_PATH = WINDOWS_EXE_PATH if os.name == "nt" else POSIX_EXE_PATH

TIMESTAMP_FORMAT = "%Y-%m-%d_%H-%M-%S_%fZ"
TIMESTAMP_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")


def _fail(failures: list[str], condition: bool, req: str, message: str) -> None:
    if not condition:
        failures.append(f"{req}: {message}")


def _run_kitchensync(
    args: list[str],
    *,
    cwd: Path,
    timeout_seconds: float = 30.0,
) -> subprocess.CompletedProcess[str] | None:
    command = [str(RELEASED_EXE_PATH), *args]
    try:
        return subprocess.run(
            command,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(
            args=command,
            returncode=124,
            stdout="",
            stderr="command timed out",
        )
    except (FileNotFoundError, OSError) as exc:
        return subprocess.CompletedProcess(
            args=command,
            returncode=127,
            stdout="",
            stderr=f"failed to launch released executable: {exc}",
        )


def _assert_exit_code_zero(
    failures: list[str],
    req: str,
    result: subprocess.CompletedProcess[str] | None,
    args: list[str],
) -> bool:
    _fail(
        failures,
        result is not None,
        req,
        f"command {args!r} failed to run or timed out",
    )
    if result is None:
        return False
    _fail(
        failures,
        result.returncode == 0,
        req,
        f"command {args!r} expected exit 0, got {result.returncode}. stdout={result.stdout!r} stderr={result.stderr!r}",
    )
    return result.returncode == 0


def _assert_empty_stderr(failures: list[str], req: str, result: subprocess.CompletedProcess[str] | None, args: list[str]) -> None:
    if result is None:
        return
    _fail(
        failures,
        not result.stderr.strip(),
        req,
        f"expected empty stderr for {args!r}, got {result.stderr!r}",
    )


def _write_text(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value, encoding="utf-8")


def _run_case(label: str, failures: list[str], fn: Callable[[], None]) -> None:
    try:
        fn()
    except AssertionError as exc:
        failures.append(f"{label}: assertion failed: {exc}")
    except Exception as exc:  # noqa: BLE001
        failures.append(f"{label}: unexpected exception: {exc!r}")


def _swap_dir(peer_root: Path, filename: str) -> Path:
    return peer_root / ".kitchensync" / "SWAP" / quote(filename, safe="")


def _swap_new(peer_root: Path, filename: str) -> Path:
    return _swap_dir(peer_root, filename) / "new"


def _swap_old(peer_root: Path, filename: str) -> Path:
    return _swap_dir(peer_root, filename) / "old"


def _prepare_swap_entry(peer_root: Path, filename: str, *, old: str | None, new: str | None) -> None:
    if old is not None:
        _write_text(_swap_old(peer_root, filename), old)
    if new is not None:
        _write_text(_swap_new(peer_root, filename), new)


def _latest_backup_entries(peer_root: Path, basename: str) -> list[Path]:
    bak_root = peer_root / ".kitchensync" / "BAK"
    if not bak_root.is_dir():
        return []
    entries: list[Path] = []
    for timestamp_dir in sorted(bak_root.iterdir(), key=lambda p: p.name):
        if not timestamp_dir.is_dir():
            continue
        candidate = timestamp_dir / basename
        if candidate.exists():
            entries.append(candidate)
    return entries


def _format_timestamp(days_ago: int) -> str:
    value = datetime.now(timezone.utc) - timedelta(days=days_ago)
    return value.strftime(TIMESTAMP_FORMAT)


def _write_meta_dir(peer_root: Path, meta_name: str, *, days_ago: int, marker: str) -> Path:
    root = peer_root / ".kitchensync" / meta_name
    ts = _format_timestamp(days_ago)
    path = root / ts
    path.mkdir(parents=True, exist_ok=True)
    _write_text(path / marker, f"{meta_name}-{marker}")
    return path


def _has_meta_marker(peer_root: Path, meta_name: str, marker: str) -> bool:
    root = peer_root / ".kitchensync" / meta_name
    if not root.is_dir():
        return False
    for timestamp_dir in sorted(root.iterdir()):
        if not timestamp_dir.is_dir():
            continue
        if (timestamp_dir / marker).is_file():
            return True
    return False


def _clean_directory(path: Path) -> None:
    if path.exists():
        if path.is_dir():
            shutil.rmtree(path)
        else:
            path.unlink()


def _ensure_parent_prepared(root: Path) -> tuple[Path, Path]:
    canon = root / "canon"
    peer = root / "peer"
    canon.mkdir(parents=True, exist_ok=True)
    peer.mkdir(parents=True, exist_ok=True)
    return canon, peer


def _seed_anchor_root(canon: Path, peer: Path, file_name: str = "anchor.txt", content: str = "anchor") -> subprocess.CompletedProcess[str] | None:
    _write_text(canon / file_name, content)
    return _run_kitchensync([f"+{canon}", str(peer)], cwd=canon.parent)


def check_displacements_are_per_parent(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_011_disp_") as raw_root:
        root = Path(raw_root)
        canon, peer = _ensure_parent_prepared(root)

        _write_text(canon / "keep.txt", "keep")
        _write_text(canon / "nested" / "keep.txt", "keep-nested")
        baseline = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)
        _assert_exit_code_zero(failures, "011.1/011.3/011.4/011.5/011.6", baseline, [f"+{canon}", str(peer)])
        _assert_empty_stderr(failures, "011.1/011.3/011.4/011.5/011.6", baseline, [f"+{canon}", str(peer)])
        if baseline is None or baseline.returncode != 0:
            return

        _write_text(peer / "delete-me.txt", "remove me")
        _write_text(peer / "nested" / "extra.txt", "remove this too")
        (peer / "type_conflict").mkdir()
        _write_text(peer / "type_conflict" / "old-child.txt", "peer-dir")
        _write_text(canon / "type_conflict", "canon-file")

        follow_up = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)
        if not _assert_exit_code_zero(failures, "011.1/011.3/011.4/011.5/011.6", follow_up, [f"+{canon}", str(peer)]):
            return
        _assert_empty_stderr(failures, "011.1/011.3/011.4/011.5/011.6", follow_up, [f"+{canon}", str(peer)])

        _fail(
            failures,
            not (peer / "delete-me.txt").exists(),
            "011.1/011.3/011.4",
            "root peer-only file was not displaced",
        )
        _fail(
            failures,
            not (peer / "nested" / "extra.txt").exists(),
            "011.1/011.5",
            "nested peer-only file was not displaced",
        )

        root_bak = _latest_backup_entries(peer, "delete-me.txt")
        _fail(failures, bool(root_bak), "011.3/011.4", "BAK entry was not created for root peer-only file")
        if root_bak:
            _fail(
                failures,
                root_bak[-1].read_text(encoding="utf-8") == "remove me",
                "011.4",
                "root BAK entry for deleted file did not preserve prior file content",
            )
            _fail(failures, root_bak[-1].name == "delete-me.txt", "011.4", "BAK entry basename was rewritten during displacement")

        nested_bak = _latest_backup_entries(peer / "nested", "extra.txt")
        _fail(failures, bool(nested_bak), "011.5", "nested BAK entry was not created under nested parent directory")
        if nested_bak:
            _fail(
                failures,
                nested_bak[-1].name == "extra.txt",
                "011.5",
                "nested BAK entry basename was not preserved",
            )
            _fail(
                failures,
                nested_bak[-1].is_file(),
                "011.5",
                "nested BAK entry for displaced file was not a file",
            )

        _fail(
            failures,
            (peer / "type_conflict").is_file(),
            "011.6",
            "type conflict entry was not replaced by file from canon",
        )
        _fail(
            failures,
            _latest_backup_entries(peer, "type_conflict"),
            "011.6",
            "type conflict directory was not moved to BAK before replacement",
        )
        conflict_bak = _latest_backup_entries(peer, "type_conflict")
        if conflict_bak:
            _fail(
                failures,
                conflict_bak[-1].is_dir(),
                "011.6",
                "type conflict BAK entry was not preserved as a directory",
            )
            _fail(
                failures,
                (conflict_bak[-1] / "old-child.txt").is_file(),
                "011.6",
                "type conflict BAK directory did not preserve subtree",
            )


def _run_recovery_case(
    failures: list[str],
    req: str,
    root: Path,
    *,
    canon_target: str | None,
    peer_target: str | None,
    target_name: str,
    swap_old: str | None,
    swap_new: str | None,
    expected_content: str | None,
    expect_old_bak: bool,
    expect_target_exists: bool,
    expect_target_content: str | None,
    expect_swap_removed: bool = True,
) -> None:
    canon, peer = _ensure_parent_prepared(root)

    if canon_target is not None:
        _write_text(canon / target_name, canon_target)

    baseline = _seed_anchor_root(canon, peer)
    _assert_exit_code_zero(
        failures,
        req,
        baseline,
        [f"+{canon}", str(peer)],
    )
    if baseline is None or baseline.returncode != 0:
        return

    if peer_target is None:
        _clean_directory(peer / target_name)
    else:
        _write_text(peer / target_name, peer_target)

    _prepare_swap_entry(peer, target_name, old=swap_old, new=swap_new)

    result = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)
    if not _assert_exit_code_zero(failures, req, result, [f"+{canon}", str(peer)]):
        return
    _assert_empty_stderr(failures, req, result, [f"+{canon}", str(peer)])

    if expect_target_exists:
        _fail(failures, (peer / target_name).exists(), req, f"expected target {target_name!r} to exist after swap recovery")
        if expect_target_content is not None and (peer / target_name).exists():
            _fail(
                failures,
                (peer / target_name).read_text(encoding="utf-8") == expect_target_content,
                req,
                f"unexpected target content for {target_name!r} after swap recovery",
            )
    else:
        _fail(failures, not (peer / target_name).exists(), req, f"expected target {target_name!r} to be absent after swap recovery")

    bak_entries = _latest_backup_entries(peer, target_name)
    if expect_old_bak:
        _fail(failures, bool(bak_entries), req, f"expected old entry for {target_name!r} to move to BAK")
        if bak_entries and expected_content is not None:
            _fail(
                failures,
                bak_entries[-1].read_text(encoding="utf-8") == expected_content,
                req,
                f"BAK payload for {target_name!r} did not match expected old content",
            )
    else:
        _fail(failures, not bak_entries, req, f"did not expect BAK entry for {target_name!r} in this swap scenario")

    swap_path = _swap_dir(peer, target_name)
    if expect_swap_removed:
        _fail(failures, not swap_path.exists(), req, f"swap directory {swap_path} was not cleaned after successful swap recovery")


def check_swap_recovery_cases(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_011_swap_case_") as raw_root:
        root = Path(raw_root)
        _run_recovery_case(
            failures,
            "011.17",
            root,
            canon_target="canon-old-target",
            peer_target="canon-old-target",
            target_name="swap-old-target.txt",
            swap_old="old-from-recovery",
            swap_new=None,
            expected_content="old-from-recovery",
            expect_old_bak=True,
            expect_target_exists=True,
            expect_target_content="canon-old-target",
        )

    with tempfile.TemporaryDirectory(prefix="ks_011_swap_case_") as raw_root:
        root = Path(raw_root)
        _run_recovery_case(
            failures,
            "011.18/011.19",
            root,
            canon_target="new-recover",
            peer_target=None,
            target_name="swap-old-new.txt",
            swap_old="old-recover",
            swap_new="new-recover",
            expected_content="old-recover",
            expect_old_bak=True,
            expect_target_exists=True,
            expect_target_content="new-recover",
        )

    with tempfile.TemporaryDirectory(prefix="ks_011_swap_case_") as raw_root:
        root = Path(raw_root)
        _run_recovery_case(
            failures,
            "011.20",
            root,
            canon_target="restore-from-old",
            peer_target=None,
            target_name="swap-old-only.txt",
            swap_old="restore-from-old",
            swap_new=None,
            expected_content=None,
            expect_old_bak=False,
            expect_target_exists=True,
            expect_target_content="restore-from-old",
        )

    with tempfile.TemporaryDirectory(prefix="ks_011_swap_case_") as raw_root:
        root = Path(raw_root)
        _run_recovery_case(
            failures,
            "011.21",
            root,
            canon_target="still-present",
            peer_target="still-present",
            target_name="swap-new-target-present.txt",
            swap_old=None,
            swap_new="new-stale-ignore",
            expected_content=None,
            expect_old_bak=False,
            expect_target_exists=True,
            expect_target_content="still-present",
        )

    with tempfile.TemporaryDirectory(prefix="ks_011_swap_case_") as raw_root:
        root = Path(raw_root)
        _run_recovery_case(
            failures,
            "011.22",
            root,
            canon_target="rename-me",
            peer_target=None,
            target_name="swap-new-only.txt",
            swap_old=None,
            swap_new="rename-me",
            expected_content=None,
            expect_old_bak=False,
            expect_target_exists=True,
            expect_target_content="rename-me",
        )


def check_encoded_basename_swap_and_recovery_scope(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_011_swap_encoded_") as raw_root:
        root = Path(raw_root)
        canon, peer = _ensure_parent_prepared(root)

        baseline = _seed_anchor_root(canon, peer)
        _assert_exit_code_zero(failures, "011.11/011.12/011.13/011.14/011.15", baseline, [f"+{canon}", str(peer)])
        if baseline is None or baseline.returncode != 0:
            return

        encoded_name = "spaced name.txt"
        encoded_target = _swap_dir(peer, encoded_name)
        _write_text(canon / encoded_name, "encoded-recovered")
        _prepare_swap_entry(peer, encoded_name, old=None, new="encoded-recovered")
        result = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)
        _assert_exit_code_zero(failures, "011.11/011.12/011.13", result, [f"+{canon}", str(peer)])

        _fail(
            failures,
            (peer / encoded_name).is_file(),
            "011.11/011.12/011.13",
            "encoded basename swap recovery did not materialize target at expected path",
        )
        _fail(
            failures,
            not encoded_target.exists(),
            "011.11/011.12/011.13",
            "encoded swap directory was not cleaned after successful recovery",
        )
        _fail(
            failures,
            (peer / encoded_name).exists()
            and (peer / encoded_name).read_text(encoding="utf-8") == "encoded-recovered",
            "011.12",
            "encoded SWAP new content was not promoted to target",
        )

        # also confirm 011.15 by creating two swap children and recovering both in one traversal
        _write_text(_swap_new(peer, "multi-a.txt"), "from-swap-a")
        _write_text(_swap_new(peer, "multi-b.txt"), "from-swap-b")
        _run_kitchensync([f"+{canon}", str(peer)], cwd=root)

        _fail(failures, not _swap_dir(peer, "multi-a.txt").exists(), "011.15", "multi swap child for a remained after traversal")
        _fail(failures, not _swap_dir(peer, "multi-b.txt").exists(), "011.15", "multi swap child for b remained after traversal")


def check_dry_run_skips_swap_recovery(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_011_swap_dryrun_") as raw_root:
        root = Path(raw_root)
        canon, peer = _ensure_parent_prepared(root)

        _seed_anchor_root(canon, peer)
        _prepare_swap_entry(peer, "dry-run-skip.txt", old=None, new="should-stay")
        result = _run_kitchensync(["--dry-run", f"+{canon}", str(peer)], cwd=root)
        _assert_exit_code_zero(failures, "011.16", result, ["--dry-run", f"+{canon}", str(peer)])

        _fail(
            failures,
            _swap_dir(peer, "dry-run-skip.txt").exists(),
            "011.16",
            "swap directory was removed during --dry-run, but it should be skipped",
        )
        _fail(
            failures,
            not (peer / "dry-run-skip.txt").exists(),
            "011.16",
            "dry-run unexpectedly restored target from swap state",
        )


def check_bak_tmp_cleanup_defaults_and_override(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_011_cleanup_default_") as raw_root:
        root = Path(raw_root)
        canon, peer = _ensure_parent_prepared(root)

        _write_text(canon / "anchor.txt", "anchor")
        seed = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)
        _assert_exit_code_zero(failures, "011.28/011.29/011.30/011.31/011.32/011.33/011.34", seed, [f"+{canon}", str(peer)])
        if seed is None or seed.returncode != 0:
            return

        _write_meta_dir(peer, "BAK", days_ago=95, marker="stale-old")
        _write_meta_dir(peer, "BAK", days_ago=1, marker="fresh")
        _write_meta_dir(peer, "TMP", days_ago=3, marker="stale-tmp")
        _write_meta_dir(peer, "TMP", days_ago=1, marker="fresh-tmp")

        sync = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)
        _assert_exit_code_zero(failures, "011.28/011.29/011.30/011.31/011.32/011.33/011.34", sync, [f"+{canon}", str(peer)])
        _fail(failures, not _has_meta_marker(peer, "BAK", marker="stale-old"), "011.30", "stale BAK directory older than keep-bak-days was not purged")
        _fail(failures, _has_meta_marker(peer, "BAK", marker="fresh"), "011.30/011.31", "fresh BAK directory was purged with default keep-bak-days")
        _fail(failures, not _has_meta_marker(peer, "TMP", marker="stale-tmp"), "011.32", "stale TMP directory older than keep-tmp-days was not purged")
        _fail(failures, _has_meta_marker(peer, "TMP", marker="fresh-tmp"), "011.33", "fresh TMP directory was purged with default keep-tmp-days")

        _write_meta_dir(peer, "BAK", days_ago=20, marker="custom-bak-old")
        _write_meta_dir(peer, "TMP", days_ago=20, marker="custom-tmp-old")
        override = _run_kitchensync(["--keep-bak-days", "5", "--keep-tmp-days", "5", f"+{canon}", str(peer)], cwd=root)
        _assert_exit_code_zero(failures, "011.30/011.33", override, ["--keep-bak-days", "5", "--keep-tmp-days", "5", f"+{canon}", str(peer)])
        _fail(failures, not _has_meta_marker(peer, "BAK", marker="custom-bak-old"), "011.30", "BAK override did not purge stale directory")
        _fail(failures, not _has_meta_marker(peer, "TMP", marker="custom-tmp-old"), "011.32", "TMP override did not purge stale directory")


def check_dry_run_skips_bak_tmp_cleanup(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_011_cleanup_dryrun_") as raw_root:
        root = Path(raw_root)
        canon, peer = _ensure_parent_prepared(root)

        _write_text(canon / "anchor.txt", "anchor")
        seed = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)
        _assert_exit_code_zero(failures, "011.35", seed, [f"+{canon}", str(peer)])
        if seed is None or seed.returncode != 0:
            return

        stale_bak = _write_meta_dir(peer, "BAK", days_ago=100, marker="dryrun-stale-bak")
        stale_tmp = _write_meta_dir(peer, "TMP", days_ago=100, marker="dryrun-stale-tmp")
        _run_kitchensync(["--dry-run", f"+{canon}", str(peer)], cwd=root)

        _fail(failures, stale_bak.exists(), "011.35", "--dry-run should not run BAK cleanup")
        _fail(failures, stale_tmp.exists(), "011.35", "--dry-run should not run TMP cleanup")


def check_displacement_failure_behavior(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_011_displace_fail_") as raw_root:
        root = Path(raw_root)
        canon, peer = _ensure_parent_prepared(root)

        _write_text(canon / "anchor.txt", "anchor")
        seed = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)
        _assert_exit_code_zero(failures, "011.38/011.39/011.40", seed, [f"+{canon}", str(peer)])
        if seed is None or seed.returncode != 0:
            return

        _write_text(peer / "displacement_fail.txt", "to remain")
        _write_text(peer / ".kitchensync" / "BAK", "blocked")

        _write_text(peer / "displacement_fail.txt", "to remain")
        bad = _run_kitchensync([f"+{canon}", str(peer)], cwd=root)
        _fail(failures, bad is not None, "011.38/011.39/011.40", "sync command failed while exercising displacement failure")
        if bad is None:
            return

        output = f"{bad.stdout}\n{bad.stderr}".lower()
        _fail(
            failures,
            "error" in output or "failed" in output,
            "011.38",
            "expected a failure message for displacement to BAK failure",
        )

        _fail(
            failures,
            (peer / "displacement_fail.txt").is_file(),
            "011.39",
            "displacement to BAK failure did not keep the original entry in place",
        )
        _fail(
            failures,
            (peer / "displacement_fail.txt").read_text(encoding="utf-8") == "to remain",
            "011.40",
            "displacement failure changed the entry rather than skipping the displacement",
        )
        _fail(
            failures,
            (peer / ".kitchensync" / "BAK").is_file(),
            "011.38/011.39/011.40",
            "displacement failure did not leave .kitchensync/BAK as an unrecovered blocker file",
        )


def main() -> int:
    failures: list[str] = []

    _fail(failures, WORKSPACE_ROOT.is_dir(), "precondition", f"workspace root missing: {WORKSPACE_ROOT}")
    _fail(failures, PROJECT_DIR.is_dir(), "precondition", f"project directory missing: {PROJECT_DIR}")
    _fail(failures, RELEASED_EXE_PATH.is_file(), "precondition", f"released executable missing: {RELEASED_EXE_PATH}")

    if failures:
        print("FAIL: test_011_displacement_and_staging_cleanup.py")
        for idx, item in enumerate(failures, start=1):
            print(f"  {idx:02d}. {item}")
        return 1

    _run_case(
        "011.1/011.3/011.4/011.5/011.6",
        failures,
        lambda: check_displacements_are_per_parent(failures),
    )
    _run_case(
        "011.17/011.18/011.19/011.20/011.21/011.22",
        failures,
        lambda: check_swap_recovery_cases(failures),
    )
    _run_case(
        "011.11/011.12/011.13/011.14/011.15",
        failures,
        lambda: check_encoded_basename_swap_and_recovery_scope(failures),
    )
    _run_case(
        "011.16",
        failures,
        lambda: check_dry_run_skips_swap_recovery(failures),
    )
    _run_case(
        "011.28/011.29/011.30/011.31/011.32/011.33/011.34",
        failures,
        lambda: check_bak_tmp_cleanup_defaults_and_override(failures),
    )
    _run_case(
        "011.35",
        failures,
        lambda: check_dry_run_skips_bak_tmp_cleanup(failures),
    )
    _run_case(
        "011.38/011.39/011.40",
        failures,
        lambda: check_displacement_failure_behavior(failures),
    )

    # not reasonably testable from this CLI-only released-product surface:
    # 011.2 -- file-copy queue contents are internal; no durable external artifact indicates where displacement originated.
    # 011.7 -- requires visibility into traversal-order internals to prove a directory was not recursed before displacement.
    # 011.8 -- requires instrumentation of per-peer traversal participation at each subtree level.
    # 011.9 -- default TMP path location is internal unless a transfer creates it; recovery assertions can only see post-cleanup state.
    # 011.10 -- per-transfer TMP UUID uniqueness is not reliably observable from final filesystem state.
    # 011.14 -- "recover before listing" is not directly observable once a correct final state is present.
    # 011.24/011.25/011.26/011.27 -- swap failure-injection at peer subtree granularity needs controlled transport/index faults.
    # 011.36 -- SWAP-age behavior can only be proven under controlled long-running swap directory states; this interface only exposes path side effects.
    # 011.37 -- requires inducing swap recovery failure while inspecting internal subtree exclusion state.

    if failures:
        print("FAIL: test_011_displacement_and_staging_cleanup.py")
        for idx, item in enumerate(failures, start=1):
            print(f"  {idx:02d}. {item}")
        return 1

    print("PASS: test_011_displacement_and_staging_cleanup.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
