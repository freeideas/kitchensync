# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""End-to-end test for --dry-run mode: reqs/024_dry-run.md."""
import subprocess
import sys
import tempfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")

_failures: list[str] = []


def fail(msg: str) -> None:
    _failures.append(msg)
    print(f"FAIL: {msg}", flush=True)


def ks(*args: object, timeout: int = 60) -> subprocess.CompletedProcess:
    cmd = [str(EXE), *(str(a) for a in args)]
    try:
        return subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        fail(f"kitchensync timed out after {timeout}s: {cmd}")
        return subprocess.CompletedProcess(cmd, returncode=-1, stdout="", stderr="")


def put(path: Path, content: str = "x\n") -> None:
    """Write content to path, creating parent directories as needed."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


with tempfile.TemporaryDirectory(prefix="ks_024_") as _td:
    base = Path(_td)

    # ──────────────────────────────────────────────────────────────────────────
    # Scenario A: basic dry run with copy work
    # Covers: 024.1 (connects to peers), 024.4 (lists peer directories),
    #         024.9 (C progress line), 024.10 (dry run phrase on stdout),
    #         024.14 (no destination files written),
    #         024.13 (no TMP/SWAP/BAK created),
    #         024.18 (no snapshot uploaded)
    # ──────────────────────────────────────────────────────────────────────────
    print("scenario A: dry run phrase, C progress, no writes", flush=True)
    a_src = base / "a_src"
    a_dst = base / "a_dst"
    a_src.mkdir()
    a_dst.mkdir()
    put(a_src / "hello.txt")

    rA = ks("--dry-run", f"+{a_src}", a_dst)

    # 024.10: "dry run" phrase on stdout
    if "dry run" not in rA.stdout:
        fail("024.10: 'dry run' not in stdout")

    # 024.9: C progress line for queued copy
    if not any(ln.startswith("C ") for ln in rA.stdout.splitlines()):
        fail("024.9: no C progress line in stdout during dry-run")

    # 024.1: connection succeeded (run completed without unreachable-peer failure)
    if rA.returncode != 0:
        fail(f"024.1: expected exit 0, got {rA.returncode}; stdout={rA.stdout!r}")

    # spec: all output goes to stdout; stderr must always be empty
    if rA.stderr:
        fail(f"stderr not empty for dry-run run: {rA.stderr!r}")

    # 024.14: no files written to destination peer
    if (a_dst / "hello.txt").exists():
        fail("024.14: hello.txt written to destination peer in dry-run")

    # 024.18 + 024.13: no .kitchensync directory created on fresh destination peer
    if (a_dst / ".kitchensync").exists():
        fail("024.18/024.13: .kitchensync created on destination peer in dry-run")

    # 024.13: no staging directories created on source peer either
    for _d in ("SWAP", "BAK", "TMP"):
        if (a_src / ".kitchensync" / _d).exists():
            fail(f"024.13: .kitchensync/{_d} created on source peer in dry-run")

    # ──────────────────────────────────────────────────────────────────────────
    # Scenario B: no directories created on peers
    # Covers: 024.12
    # ──────────────────────────────────────────────────────────────────────────
    print("scenario B: no peer directories created", flush=True)
    b_src = base / "b_src"
    b_dst = base / "b_dst"
    b_src.mkdir()
    b_dst.mkdir()
    put(b_src / "sub" / "file.txt")

    rB = ks("--dry-run", f"+{b_src}", b_dst)

    if (b_dst / "sub").exists():
        fail("024.12: subdirectory 'sub' created on destination peer in dry-run")

    # ──────────────────────────────────────────────────────────────────────────
    # Scenario C: no displacement or deletion of files
    # Covers: 024.9 (X progress line), 024.15 (no displacement),
    #         024.16 (no deletion), 024.13 (no BAK created)
    # ──────────────────────────────────────────────────────────────────────────
    print("scenario C: X progress line, no displacement", flush=True)
    c_src = base / "c_src"
    c_dst = base / "c_dst"
    c_src.mkdir()
    c_dst.mkdir()
    put(c_src / "canon.txt")
    # extra.txt is on c_dst only; canon does not have it, so it would be
    # displaced in a normal run. In dry-run it must remain.
    put(c_dst / "extra.txt")

    rC = ks("--dry-run", f"+{c_src}", c_dst)

    # 024.9: X progress line for planned displacement
    if not any(ln.startswith("X ") for ln in rC.stdout.splitlines()):
        fail("024.9: no X progress line in stdout during dry-run")

    # 024.15 / 024.16: extra.txt not displaced or deleted
    if not (c_dst / "extra.txt").exists():
        fail("024.15/024.16: extra.txt displaced or deleted from peer in dry-run")

    # 024.13: no BAK directory created by suppressed displacement
    if (c_dst / ".kitchensync" / "BAK").exists():
        fail("024.13: .kitchensync/BAK created on peer during displacement in dry-run")

    # ──────────────────────────────────────────────────────────────────────────
    # Scenario D: non-existent peer root treated as unreachable, not created
    # Covers: 024.11
    # ──────────────────────────────────────────────────────────────────────────
    print("scenario D: non-existent root is unreachable in dry-run", flush=True)
    d_src = base / "d_src"
    d_ok = base / "d_ok"
    d_miss = base / "d_miss"   # intentionally not created
    d_src.mkdir()
    d_ok.mkdir()
    put(d_src / "file.txt")

    rD = ks("--dry-run", f"+{d_src}", d_ok, d_miss)

    # 024.11: missing root directory not created in dry-run
    if d_miss.exists():
        fail("024.11: non-existent peer root directory was created in dry-run")

    # run exits 0 because d_src (canon) and d_ok are still reachable
    if rD.returncode != 0:
        fail(f"024.11: expected exit 0 with two reachable peers, got {rD.returncode}")

    # ──────────────────────────────────────────────────────────────────────────
    # Scenario E: startup snapshot SWAP recovery skipped
    # Covers: 024.2
    # Note 024.3: that the download is the live db "as-is" (no SWAP recovery
    # first) is inferred from 024.2 — if recovery ran, old would be gone.
    # ──────────────────────────────────────────────────────────────────────────
    print("scenario E: startup SWAP recovery skipped", flush=True)
    e_src = base / "e_src"
    e_dst = base / "e_dst"
    e_src.mkdir()
    e_dst.mkdir()
    # Simulate incomplete snapshot SWAP: old exists, new absent, live snapshot.db absent.
    # Normal run: old is renamed to snapshot.db (recovered before download).
    # Dry-run: recovery is skipped; old must remain untouched.
    e_swap_dir = e_src / ".kitchensync" / "SWAP" / "snapshot.db"
    e_swap_dir.mkdir(parents=True)
    e_swap_old = e_swap_dir / "old"
    put(e_swap_old, "fake snapshot bytes\n")
    put(e_src / "afile.txt")

    rE = ks("--dry-run", f"+{e_src}", e_dst)

    if not e_swap_old.exists():
        fail("024.2: SWAP/snapshot.db/old was consumed in dry-run (startup recovery ran)")

    # ──────────────────────────────────────────────────────────────────────────
    # Scenario F: BAK/TMP cleanup skipped during traversal
    # Covers: 024.19
    # ──────────────────────────────────────────────────────────────────────────
    print("scenario F: BAK/TMP cleanup skipped", flush=True)
    f_src = base / "f_src"
    f_dst = base / "f_dst"
    f_src.mkdir()
    f_dst.mkdir()
    # Timestamp far in the past so entries are well beyond any keep-*-days threshold.
    _old_ts = "2020-01-01_00-00-00_000000Z"
    f_bak = f_src / ".kitchensync" / "BAK" / _old_ts / "displaced.txt"
    put(f_bak, "old backup\n")
    f_tmp = f_src / ".kitchensync" / "TMP" / _old_ts / "staging"
    put(f_tmp, "old staging\n")
    put(f_src / "live.txt")

    rF = ks("--dry-run", f"+{f_src}", f_dst)

    # 024.19: expired BAK entry must remain (cleanup skipped)
    if not f_bak.exists():
        fail("024.19: expired BAK entry was purged in dry-run")

    # 024.19: expired TMP entry must remain (cleanup skipped)
    if not f_tmp.exists():
        fail("024.19: expired TMP entry was purged in dry-run")

    # ──────────────────────────────────────────────────────────────────────────
    # Scenario G: traversal-level SWAP recovery skipped
    # Covers: 024.20
    # ──────────────────────────────────────────────────────────────────────────
    print("scenario G: traversal SWAP recovery skipped", flush=True)
    g_src = base / "g_src"
    g_dst = base / "g_dst"
    g_src.mkdir()
    g_dst.mkdir()
    # SWAP state at root level: new present, old absent, live target present.
    # Normal recovery rule: delete new and remove the empty SWAP directory.
    # Dry-run: recovery is skipped; new must remain.
    put(g_src / "afile")
    g_swap_dir = g_src / ".kitchensync" / "SWAP" / "afile"
    g_swap_dir.mkdir(parents=True)
    g_swap_new = g_swap_dir / "new"
    put(g_swap_new, "staged replacement\n")

    rG = ks("--dry-run", f"+{g_src}", g_dst)

    if not g_swap_new.exists():
        fail("024.20: SWAP/afile/new was deleted in dry-run (traversal recovery ran)")


# not reasonably testable: 024.3  -- download is as-is (no SWAP recovery first);
#   inferred from 024.2: if startup recovery had run, e_swap_old would be gone
# not reasonably testable: 024.5  -- source-file reads in dry-run copy not
#   observable externally
# not reasonably testable: 024.6  -- local temp snapshot db creation/update
#   not observable externally
# not reasonably testable: 024.7  -- copy-slot acquisition in dry-run not
#   observable externally
# not reasonably testable: 024.8  -- retries-copy limit in dry-run not observable
#   without triggering copy failures
# not reasonably testable: 024.17 -- no mod_time set; covered by 024.14: if no
#   destination files are written, no mod_times can be set on peers

print(flush=True)
if _failures:
    print(f"FAILED: {len(_failures)} check(s):")
    for _m in _failures:
        print(f"  {_m}")
    sys.exit(1)

print("All 024_dry-run checks passed.")
sys.exit(0)
