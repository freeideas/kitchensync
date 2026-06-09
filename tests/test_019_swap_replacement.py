# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
End-to-end tests for 019_swap-replacement requirements.

Not reasonably testable (require transport failure injection):
  019.9  -- SWAP new deleted on transfer failure before old exists
  019.10 -- original destination remains when move-to-old fails
  019.11 -- copy skipped for run when move-to-old fails
  019.12 -- SWAP state left after transfer failure after old exists
  019.13 -- old left in place when archive-old to BAK fails
  019.20 -- directory listing treated as failed when SWAP recovery fails
"""

import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

import os
import subprocess
import tempfile
import time
from pathlib import Path

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"

_failures: list[str] = []


def check(ok: bool, msg: str) -> None:
    if ok:
        print(f"  PASS: {msg}")
    else:
        _failures.append(msg)
        print(f"  FAIL: {msg}")


def run_ks(*args: str, timeout: int = 30) -> subprocess.CompletedProcess:
    cmd = [str(EXE)] + list(args)
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


def make_file(path: Path, content: str | bytes, mtime: float | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(content, bytes):
        path.write_bytes(content)
    else:
        path.write_text(content, encoding="utf-8")
    if mtime is not None:
        os.utime(str(path), (mtime, mtime))


def bak_files(peer_dir: Path) -> list[Path]:
    """All files under peer_dir/.kitchensync/BAK/."""
    bak = peer_dir / ".kitchensync" / "BAK"
    if not bak.is_dir():
        return []
    return [p for p in bak.rglob("*") if p.is_file()]


def swap_root(peer_dir: Path) -> Path:
    return peer_dir / ".kitchensync" / "SWAP"


def has_swap_dirs(peer_dir: Path) -> bool:
    """True if any SWAP child dirs exist under peer_dir/.kitchensync/SWAP/."""
    sr = swap_root(peer_dir)
    if not sr.is_dir():
        return False
    return any(True for _ in sr.iterdir())


# ---------------------------------------------------------------------------
# 019.1-019.6: Full SWAP sequence when replacing an existing destination file
# ---------------------------------------------------------------------------


def test_019_1_to_6_replace_existing() -> None:
    """
    019.1 -- new content written to SWAP new before replacing target
    019.2 -- existing file moved to SWAP old before swap-in
    019.3 -- SWAP new renamed to final target path
    019.4 -- destination mod_time set to winning mod_time from decision
    019.5 -- SWAP old archived to BAK after new file is in place
    019.6 -- empty SWAP directories removed after replacement
    """
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        pa = t / "pa"
        pb = t / "pb"
        pa.mkdir()
        pb.mkdir()

        t_new = time.time() - 10
        t_old = time.time() - 100
        make_file(pa / "hello.txt", "new content", mtime=t_new)
        make_file(pb / "hello.txt", "old content", mtime=t_old)

        r = run_ks(f"+{pa}", str(pb))
        check(r.returncode == 0, f"019.1-019.6: exit 0 (got {r.returncode})")
        check(r.stderr == "", f"019.1-019.6: stderr empty (got {r.stderr!r})")

        target = pb / "hello.txt"

        # 019.3: new file in final place
        check(target.exists(), "019.3: hello.txt exists on destination after sync")
        if target.exists():
            got = target.read_text(encoding="utf-8")
            check(got == "new content", f"019.3: destination has new content (got {got!r})")

            # 019.4: mod_time set to winning mod_time (canonical peer A's value)
            a_mtime = (pa / "hello.txt").stat().st_mtime
            b_mtime = target.stat().st_mtime
            check(
                abs(b_mtime - a_mtime) < 3.0,
                f"019.4: destination mod_time matches winning mod_time "
                f"(A={a_mtime:.3f}, B={b_mtime:.3f}, diff={abs(b_mtime - a_mtime):.3f}s)",
            )

        # 019.5: old content archived to BAK (proves SWAP old was created and archived)
        bf = bak_files(pb)
        check(len(bf) >= 1, f"019.5: old file archived to BAK (found {len(bf)} entries)")
        if bf:
            bak_texts = [p.read_text(encoding="utf-8", errors="replace") for p in bf]
            check(
                any("old content" in c for c in bak_texts),
                f"019.5: BAK contains old content (found: {bak_texts})",
            )

        # 019.6: SWAP dirs cleaned up
        check(not has_swap_dirs(pb), "019.6: no SWAP dirs remain after sync")


# ---------------------------------------------------------------------------
# 019.6: SWAP cleaned when destination has no pre-existing file
# ---------------------------------------------------------------------------


def test_019_6_new_file_no_existing() -> None:
    """019.6: SWAP dirs cleaned up even when no pre-existing destination file."""
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        pa = t / "pa"
        pb = t / "pb"
        pa.mkdir()
        pb.mkdir()

        make_file(pa / "fresh.txt", "fresh")

        r = run_ks(f"+{pa}", str(pb))
        check(r.returncode == 0, f"019.6 (new): exit 0 (got {r.returncode})")
        check((pb / "fresh.txt").exists(), "019.6: fresh.txt created on destination")
        check(not has_swap_dirs(pb), "019.6: no SWAP dirs remain for new-file copy")


# ---------------------------------------------------------------------------
# 019.7: Percent-encoded basename in SWAP path segment
# ---------------------------------------------------------------------------


def test_019_7_encoded_basename_space() -> None:
    """019.7: basename with a space is handled correctly (implying percent-encoding in SWAP)."""
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        pa = t / "pa"
        pb = t / "pb"
        pa.mkdir()
        pb.mkdir()

        fname = "my file.txt"
        t_new = time.time() - 10
        make_file(pa / fname, "new", mtime=t_new)
        make_file(pb / fname, "old", mtime=t_new - 60)

        r = run_ks(f"+{pa}", str(pb))
        check(r.returncode == 0, f"019.7: exit 0 for space-named file (got {r.returncode})")

        target = pb / fname
        check(target.exists(), "019.7: space-named file exists on destination after sync")
        if target.exists():
            got = target.read_text(encoding="utf-8")
            check(got == "new", f"019.7: space-named file replaced correctly (got {got!r})")

        check(not has_swap_dirs(pb), "019.7: SWAP dirs cleaned after space-named file sync")
        bf = bak_files(pb)
        check(len(bf) >= 1, "019.7: old space-named file archived to BAK (SWAP sequence ran)")


def test_019_7_encoded_basename_recovery() -> None:
    """019.7, 019.14: SWAP recovery uses percent-encoded directory name to locate SWAP dirs."""
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        pa = t / "pa"
        pb = t / "pb"
        pa.mkdir()
        pb.mkdir()

        fname = "my file.txt"
        make_file(pa / fname, "from A")

        # Pre-create SWAP using the percent-encoded name for "my file.txt" -> "my%20file.txt"
        encoded = "my%20file.txt"
        swap_dir = pb / ".kitchensync" / "SWAP" / encoded
        swap_dir.mkdir(parents=True)
        (swap_dir / "new").write_bytes(b"orphaned new")
        # No target on pb -> recovery case 019.19: rename new -> target

        r = run_ks(f"+{pa}", str(pb))
        check(r.returncode == 0, f"019.7 (recovery): exit 0 (got {r.returncode})")

        # Encoded SWAP dir should be gone after recovery
        check(not swap_dir.exists(), "019.7: percent-encoded SWAP dir cleaned after recovery")
        check((pb / fname).exists(), f"019.7: '{fname}' exists on destination after recovery")


# ---------------------------------------------------------------------------
# 019.14, 019.15-019.19: SWAP recovery states
# ---------------------------------------------------------------------------


def test_019_15_recovery_old_target_exists() -> None:
    """019.14, 019.15: old exists and target exists -> move old to BAK, remove SWAP dir."""
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        pa = t / "pa"
        pb = t / "pb"
        pa.mkdir()
        pb.mkdir()

        make_file(pa / "photo.jpg", "from A")
        make_file(pb / "photo.jpg", "current B")

        swap_dir = pb / ".kitchensync" / "SWAP" / "photo.jpg"
        swap_dir.mkdir(parents=True)
        (swap_dir / "old").write_bytes(b"orphaned old")

        r = run_ks(f"+{pa}", str(pb))
        check(r.returncode == 0, f"019.15: exit 0 (got {r.returncode})")

        check(not swap_dir.exists(), "019.15: SWAP dir removed after recovery")

        bf = bak_files(pb)
        check(len(bf) >= 1, "019.15: SWAP old moved to BAK during recovery")
        if bf:
            bak_bytes = [p.read_bytes() for p in bf]
            check(
                any(c == b"orphaned old" for c in bak_bytes),
                "019.15: BAK contains recovered old content",
            )


def test_019_16_recovery_old_new_target_missing() -> None:
    """019.16: old+new exist, target missing -> rename new to target, move old to BAK."""
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        pa = t / "pa"
        pb = t / "pb"
        pa.mkdir()
        pb.mkdir()

        make_file(pa / "photo.jpg", "from A")

        swap_dir = pb / ".kitchensync" / "SWAP" / "photo.jpg"
        swap_dir.mkdir(parents=True)
        (swap_dir / "old").write_bytes(b"old version")
        (swap_dir / "new").write_bytes(b"new from swap")
        # No photo.jpg on pb

        r = run_ks(f"+{pa}", str(pb))
        check(r.returncode == 0, f"019.16: exit 0 (got {r.returncode})")

        check(not swap_dir.exists(), "019.16: SWAP dir removed after recovery")
        check((pb / "photo.jpg").exists(), "019.16: photo.jpg exists after recovery")

        bf = bak_files(pb)
        check(len(bf) >= 1, "019.16: at least one BAK entry (old from recovery)")
        if bf:
            bak_bytes = [p.read_bytes() for p in bf]
            check(
                any(c == b"old version" for c in bak_bytes),
                "019.16: BAK contains recovered old version",
            )


def test_019_17_recovery_old_only_target_missing() -> None:
    """019.17: only old exists, target missing -> rename old back to target, remove SWAP dir."""
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        pa = t / "pa"
        pb = t / "pb"
        pa.mkdir()
        pb.mkdir()

        make_file(pa / "photo.jpg", "from A")

        swap_dir = pb / ".kitchensync" / "SWAP" / "photo.jpg"
        swap_dir.mkdir(parents=True)
        (swap_dir / "old").write_bytes(b"rescued old")
        # No new, no target

        r = run_ks(f"+{pa}", str(pb))
        check(r.returncode == 0, f"019.17: exit 0 (got {r.returncode})")

        check(not swap_dir.exists(), "019.17: SWAP dir removed after recovery (old -> target)")
        check((pb / "photo.jpg").exists(), "019.17: photo.jpg exists after old rescued back to target")


def test_019_18_recovery_new_only_target_exists() -> None:
    """019.18: only new exists, target exists -> delete new, remove SWAP dir."""
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        pa = t / "pa"
        pb = t / "pb"
        pa.mkdir()
        pb.mkdir()

        make_file(pa / "photo.jpg", "from A")
        make_file(pb / "photo.jpg", "current B")

        swap_dir = pb / ".kitchensync" / "SWAP" / "photo.jpg"
        swap_dir.mkdir(parents=True)
        (swap_dir / "new").write_bytes(b"orphaned new")
        # No old

        r = run_ks(f"+{pa}", str(pb))
        check(r.returncode == 0, f"019.18: exit 0 (got {r.returncode})")

        check(not swap_dir.exists(), "019.18: SWAP dir removed after recovery (orphaned new deleted)")
        check((pb / "photo.jpg").exists(), "019.18: photo.jpg still present after recovery")


def test_019_19_recovery_new_only_target_missing() -> None:
    """019.19: only new exists, target missing -> rename new to target, remove SWAP dir."""
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        pa = t / "pa"
        pb = t / "pb"
        pa.mkdir()
        pb.mkdir()

        make_file(pa / "photo.jpg", "from A")

        swap_dir = pb / ".kitchensync" / "SWAP" / "photo.jpg"
        swap_dir.mkdir(parents=True)
        (swap_dir / "new").write_bytes(b"orphaned new becoming target")
        # No old, no target

        r = run_ks(f"+{pa}", str(pb))
        check(r.returncode == 0, f"019.19: exit 0 (got {r.returncode})")

        check(not swap_dir.exists(), "019.19: SWAP dir removed after recovery (new -> target)")
        check((pb / "photo.jpg").exists(), "019.19: photo.jpg exists after recovery and sync")


# ---------------------------------------------------------------------------
# 019.8: Pre-existing SWAP recovered before listing (inferred from 019.14-019.19)
# ---------------------------------------------------------------------------


def test_019_8_existing_swap_recovered_before_listing() -> None:
    """019.8: KitchenSync recovers existing SWAP before starting a replacement."""
    # Recovery preceding listing is verified by the 019.15-019.19 tests: if
    # recovery did not run before listing, the pre-created SWAP state would be
    # left in place (observable via has_swap_dirs).  Those tests confirm
    # recovery ran by verifying SWAP dirs are gone and files are in the
    # expected state.  019.8 has no distinct observable beyond those cases;
    # this stub documents the coverage mapping.
    pass


# ---------------------------------------------------------------------------
# 019.21: --dry-run skips peer-side SWAP recovery
# ---------------------------------------------------------------------------


def test_019_21_dry_run_skips_swap_recovery() -> None:
    """019.21: In --dry-run, SWAP recovery during traversal is skipped."""
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        pa = t / "pa"
        pb = t / "pb"
        pa.mkdir()
        pb.mkdir()

        make_file(pa / "photo.jpg", "from A")

        # Pre-create SWAP state on pb (case 019.19: new only, no target)
        swap_dir = pb / ".kitchensync" / "SWAP" / "photo.jpg"
        swap_dir.mkdir(parents=True)
        (swap_dir / "new").write_bytes(b"orphaned new")

        r = run_ks("--dry-run", f"+{pa}", str(pb))
        check(r.returncode == 0, f"019.21: dry-run exit 0 (got {r.returncode})")

        # SWAP state must be left untouched (recovery skipped in dry-run)
        check(swap_dir.exists(), "019.21: SWAP dir not cleaned in dry-run (recovery skipped)")
        check((swap_dir / "new").exists(), "019.21: SWAP new not removed in dry-run")

        # Target must not be created (dry-run makes no peer changes)
        check(
            not (pb / "photo.jpg").exists(),
            "019.21: target not created on destination in dry-run",
        )

        # Dry-run output must mention 'dry run' (spec requirement)
        check(
            "dry run" in r.stdout.lower(),
            "019.21: output includes 'dry run' phrase",
        )


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------


def main() -> None:
    tests = [
        test_019_1_to_6_replace_existing,
        test_019_6_new_file_no_existing,
        test_019_7_encoded_basename_space,
        test_019_7_encoded_basename_recovery,
        test_019_8_existing_swap_recovered_before_listing,
        test_019_15_recovery_old_target_exists,
        test_019_16_recovery_old_new_target_missing,
        test_019_17_recovery_old_only_target_missing,
        test_019_18_recovery_new_only_target_exists,
        test_019_19_recovery_new_only_target_missing,
        test_019_21_dry_run_skips_swap_recovery,
    ]

    for fn in tests:
        print(f"\n=== {fn.__name__} ===")
        try:
            fn()
        except subprocess.TimeoutExpired:
            msg = f"{fn.__name__}: timed out"
            _failures.append(msg)
            print(f"  TIMEOUT: {msg}")
        except Exception as exc:
            msg = f"{fn.__name__}: unexpected exception: {exc}"
            _failures.append(msg)
            print(f"  EXCEPTION: {exc}")

    print(f"\n{'=' * 60}")
    if _failures:
        print(f"FAILED ({len(_failures)} failure(s)):")
        for f in _failures:
            print(f"  - {f}")
        sys.exit(1)
    else:
        print("All checks passed.")
        sys.exit(0)


if __name__ == "__main__":
    main()
