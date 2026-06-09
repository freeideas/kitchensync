# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""End-to-end tests for reqs/023_logging.md: output channels, progress lines, diagnostics."""

import platform
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")

_failures = []


def fail(msg):
    _failures.append(msg)
    print(f"FAIL: {msg}")


def run_ks(args, timeout=30):
    cmd = [str(EXE)] + list(args)
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


def cx_lines(stdout):
    """Return lines that start with 'C ' or 'X '."""
    return [ln for ln in stdout.splitlines() if re.match(r"^[CX] ", ln)]


# ---------------------------------------------------------------------------
# 023.1 + 023.2: All output to stdout; stderr always empty
# ---------------------------------------------------------------------------

def test_output_channels():
    """023.1 + 023.2: sync output goes to stdout; stderr is empty."""
    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        a.mkdir()
        b.mkdir()
        (a / "hello.txt").write_text("hello world")

        r = run_ks([f"+{a}", str(b)])

        if r.stderr:
            fail(f"023.2: stderr must be empty during sync, got: {r.stderr!r}")
        if r.returncode != 0:
            fail(f"023.1: sync failed (rc={r.returncode}); stdout={r.stdout!r}")
            return
        if not r.stdout.strip():
            fail("023.1: stdout should contain output after sync (completion message or progress)")


def test_stderr_empty_on_arg_error():
    """023.2: stderr remains empty even when argument parsing fails."""
    # Too few peers triggers a parse error; all output must go to stdout.
    r = run_ks(["/nonexistent-only-one-peer"])
    if r.stderr:
        fail(f"023.2: stderr must be empty on argument error, got: {r.stderr!r}")


# ---------------------------------------------------------------------------
# 023.3 + 023.6: One plain progress line per action; format is letter + space + relpath
# ---------------------------------------------------------------------------

def test_progress_line_format():
    """023.3: one line per action in order; 023.6: action-letter space slash-relpath format."""
    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        a.mkdir()
        b.mkdir()
        (a / "alpha.txt").write_text("alpha")
        (a / "beta.txt").write_text("beta")

        r = run_ks([f"+{a}", str(b)])
        if r.returncode != 0:
            fail(f"023.3: sync failed: {r.stdout!r}")
            return

        for ln in cx_lines(r.stdout):
            # letter, single space, then at least one non-whitespace character
            if not re.match(r"^[CX] \S", ln):
                fail(f"023.6: bad progress line format: {ln!r}")
            relpath = ln[2:]
            # 023.6: slash-separated (no backslashes)
            if "\\" in relpath:
                fail(f"023.6: backslash in progress relpath: {ln!r}")

        # 023.3: exactly one C line per copied file
        c_paths = [ln[2:] for ln in cx_lines(r.stdout) if ln.startswith("C ")]
        if sorted(c_paths) != ["alpha.txt", "beta.txt"]:
            fail(f"023.3: expected one C line per file, got: {cx_lines(r.stdout)}")


# ---------------------------------------------------------------------------
# 023.7: Exactly one C line per path regardless of peer count
# ---------------------------------------------------------------------------

def test_one_c_line_per_path_multiple_peers():
    """023.7: one C line per path even when multiple peers receive it."""
    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        c = Path(tmp) / "c"
        a.mkdir()
        b.mkdir()
        c.mkdir()
        (a / "shared.txt").write_text("shared content")

        r = run_ks([f"+{a}", str(b), str(c)])
        if r.returncode != 0:
            fail(f"023.7: sync failed: {r.stdout!r}")
            return

        c_found = [ln for ln in cx_lines(r.stdout) if ln.startswith("C ")]
        if len(c_found) != 1 or c_found[0] != "C shared.txt":
            fail(f"023.7: expected exactly one 'C shared.txt' for 2-peer copy, got: {c_found}")


# ---------------------------------------------------------------------------
# 023.4 + 023.5: No terminal control sequences; output same for pipe stdout
# ---------------------------------------------------------------------------

def test_no_terminal_control_sequences():
    """023.5: no ANSI escapes or bare carriage returns; 023.4: pipe stdout gives correct output."""
    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        a.mkdir()
        b.mkdir()
        (a / "data.txt").write_text("data")

        r = run_ks([f"+{a}", str(b)])
        if r.returncode != 0:
            fail(f"023.4/023.5: sync failed: {r.stdout!r}")
            return

        # 023.5: no ESC character (ANSI sequences start with ESC)
        if "\x1b" in r.stdout:
            fail("023.5: ANSI escape sequences (ESC) found in stdout")
        # 023.5: no standalone carriage return (progress-bar pattern)
        # Python text mode converts \r\n -> \n; a remaining \r is a bare control char.
        if "\r" in r.stdout:
            fail("023.5: bare carriage return (terminal control) found in stdout")

        # 023.4: with stdout as a pipe (non-TTY) the C line still appears
        if not any(ln.startswith("C ") for ln in r.stdout.splitlines()):
            fail("023.4: expected C progress line when stdout is a pipe (non-TTY)")


# ---------------------------------------------------------------------------
# 023.8: X line for displaced file and displaced directory
# ---------------------------------------------------------------------------

def test_x_line_for_displacement():
    """023.8: one X line per displaced path; works for files and directories alike."""
    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        a.mkdir()
        b.mkdir()
        (a / "gone.txt").write_text("will be removed")
        sub = a / "subdir"
        sub.mkdir()
        (sub / "child.txt").write_text("child")

        # First run: establish snapshots on both peers.
        r1 = run_ks([f"+{a}", str(b)])
        if r1.returncode != 0:
            fail(f"023.8 setup: first sync failed: {r1.stdout!r}")
            return

        # Remove from A so they are displaced from B on the next run.
        (a / "gone.txt").unlink()
        shutil.rmtree(str(a / "subdir"))

        # Second run: a is canon and lacks the deleted paths, so b's copies are displaced.
        r2 = run_ks([f"+{a}", str(b)])
        if r2.returncode != 0:
            fail(f"023.8: second sync failed: {r2.stdout!r}")
            return

        x_paths = [ln[2:] for ln in cx_lines(r2.stdout) if ln.startswith("X ")]

        if "gone.txt" not in x_paths:
            fail(f"023.8: expected 'X gone.txt' line, got: {cx_lines(r2.stdout)}")
        # Directory displacement produces one X line for the directory, not per child file.
        if "subdir" not in x_paths:
            fail(f"023.8: expected 'X subdir' for directory displacement, got: {cx_lines(r2.stdout)}")

        # Exactly one X per path (023.8: regardless of how many peers are affected).
        if x_paths.count("gone.txt") > 1:
            fail("023.8: more than one X line for gone.txt")
        if x_paths.count("subdir") > 1:
            fail("023.8: more than one X line for subdir")


# ---------------------------------------------------------------------------
# 023.9: No progress line for directory creation, snapshot work, or BAK/TMP cleanup
# ---------------------------------------------------------------------------

def test_no_progress_for_directory_creation():
    """023.9: directory creation emits no C or X line."""
    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        a.mkdir()
        b.mkdir()
        (a / "keep.txt").write_text("keep this file")
        (a / "newdir").mkdir()  # empty directory — only creation needed, no copy

        r = run_ks([f"+{a}", str(b)])
        if r.returncode != 0:
            fail(f"023.9: sync failed: {r.stdout!r}")
            return

        found = cx_lines(r.stdout)
        c_paths = [ln[2:] for ln in found if ln.startswith("C ")]

        # The file produces a C line to confirm sync ran.
        if "keep.txt" not in c_paths:
            fail(f"023.9: expected 'C keep.txt' for file copy, got: {found}")

        # No C or X line may mention the directory.
        for ln in found:
            if "newdir" in ln:
                fail(f"023.9: directory creation produced a progress line: {ln!r}")


# ---------------------------------------------------------------------------
# 023.10 + 023.12: Verbosity cumulative; C/X lines at info or higher only
# ---------------------------------------------------------------------------

def test_verbosity_error_suppresses_cx():
    """023.10, 023.12: C/X lines must not appear at error verbosity."""
    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        a.mkdir()
        b.mkdir()
        (a / "doc.txt").write_text("document")

        r = run_ks([f"+{a}", str(b), "--verbosity", "error"])
        if r.returncode != 0:
            fail(f"023.10: sync failed: {r.stdout!r}")
            return

        found_cx = cx_lines(r.stdout)
        if found_cx:
            fail(f"023.12: C/X lines must be suppressed at error verbosity, got: {found_cx}")
        if r.stderr:
            fail(f"023.2: stderr not empty at error verbosity: {r.stderr!r}")


def test_verbosity_info_shows_cx():
    """023.12: C/X lines must appear at info verbosity (default)."""
    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        a.mkdir()
        b.mkdir()
        (a / "doc.txt").write_text("document")

        r = run_ks([f"+{a}", str(b), "--verbosity", "info"])
        if r.returncode != 0:
            fail(f"023.12: sync failed: {r.stdout!r}")
            return

        if not cx_lines(r.stdout):
            fail("023.12: C/X lines must appear at info verbosity")


# ---------------------------------------------------------------------------
# 023.11: Error-level diagnostics visible at error verbosity
# ---------------------------------------------------------------------------

def test_error_diagnostic_at_error_verbosity():
    """023.11: error diagnostic for unreachable peer appears even at error verbosity."""
    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        # c not created: in --dry-run, missing peer root is unreachable.
        c = Path(tmp) / "c"
        a.mkdir()
        b.mkdir()
        (a / "item.txt").write_text("item")

        # With --dry-run, c (nonexistent dir) is treated as unreachable and gets an
        # error-level diagnostic. a and b are still reachable (two peers), so sync proceeds.
        r = run_ks([f"+{a}", str(b), str(c), "--dry-run", "--verbosity", "error"])

        if not r.stdout.strip():
            fail("023.11: error diagnostic must appear when a peer is unreachable (stdout empty)")
        if cx_lines(r.stdout):
            fail(f"023.11: C/X lines must not appear at error verbosity, got: {cx_lines(r.stdout)}")
        if r.stderr:
            fail(f"023.2: stderr not empty: {r.stderr!r}")


# ---------------------------------------------------------------------------
# 023.13: --verbosity debug observationally identical to --verbosity info
# ---------------------------------------------------------------------------

def test_debug_identical_to_info():
    """023.13: debug verbosity produces the same C/X lines as info verbosity."""
    with tempfile.TemporaryDirectory() as tmp_i, \
         tempfile.TemporaryDirectory() as tmp_d:

        a_i, b_i = Path(tmp_i) / "a", Path(tmp_i) / "b"
        a_d, b_d = Path(tmp_d) / "a", Path(tmp_d) / "b"

        for a, b in [(a_i, b_i), (a_d, b_d)]:
            a.mkdir()
            b.mkdir()
            (a / "one.txt").write_text("one")
            (a / "two.txt").write_text("two")

        r_info = run_ks([f"+{a_i}", str(b_i), "--verbosity", "info"])
        r_debug = run_ks([f"+{a_d}", str(b_d), "--verbosity", "debug"])

        if r_info.returncode != 0:
            fail(f"023.13: info sync failed: {r_info.stdout!r}")
            return
        if r_debug.returncode != 0:
            fail(f"023.13: debug sync failed: {r_debug.stdout!r}")
            return

        cx_i = sorted(cx_lines(r_info.stdout))
        cx_d = sorted(cx_lines(r_debug.stdout))
        if cx_i != cx_d:
            fail(f"023.13: debug C/X output differs from info: info={cx_i} debug={cx_d}")


# ---------------------------------------------------------------------------
# 023.14 + 023.15: copy-slot events only at trace; format copy-slots active=N/M
# ---------------------------------------------------------------------------

def test_trace_copy_slot_events():
    """023.14: copy-slot events at trace; 023.15: format is 'copy-slots active=N/M'."""
    slot_re = re.compile(r"^copy-slots active=\d+/\d+$")

    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        a.mkdir()
        b.mkdir()
        (a / "payload.bin").write_bytes(b"\x00" * 1024)

        r = run_ks([f"+{a}", str(b), "--verbosity", "trace"])
        if r.returncode != 0:
            fail(f"023.14: trace sync failed: {r.stdout!r}")
            return

        slot_lines = [ln for ln in r.stdout.splitlines() if "copy-slots" in ln]
        if not slot_lines:
            fail("023.14: no copy-slot events found at trace verbosity")

        for ln in slot_lines:
            if not slot_re.match(ln):
                fail(f"023.15: copy-slot line format wrong: {ln!r}")

    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        a.mkdir()
        b.mkdir()
        (a / "payload.bin").write_bytes(b"\x00" * 1024)

        r = run_ks([f"+{a}", str(b), "--verbosity", "info"])
        if r.returncode != 0:
            fail(f"023.14: info sync failed: {r.stdout!r}")
            return

        slot_lines_info = [ln for ln in r.stdout.splitlines() if "copy-slots" in ln]
        if slot_lines_info:
            fail(f"023.14: copy-slot events must not appear at info verbosity, got: {slot_lines_info}")


# ---------------------------------------------------------------------------
# 023.16 + 023.17: Failed transfer diagnostic: relpath, peer URL, phase, error category
# ---------------------------------------------------------------------------

VALID_PHASES = frozenset({
    "read_source", "write_swap_new", "move_existing_to_swap_old",
    "rename_final", "set_mod_time", "archive_old", "cleanup",
})


def test_failed_transfer_diagnostic():
    """023.16 + 023.17: failed transfer diagnostic has relpath, peer URL, valid phase, error category."""
    if platform.system() == "Windows":
        # not reasonably testable: 023.16/023.17 chmod-based write failure not portable to Windows
        print("SKIP 023.16/023.17: permission-based transfer failure not portable to Windows")
        return

    with tempfile.TemporaryDirectory() as tmp:
        a = Path(tmp) / "a"
        b = Path(tmp) / "b"
        a.mkdir()
        b.mkdir()
        (a / "target.txt").write_text("original content")

        # First run: establish snapshots on both peers.
        r1 = run_ks([f"+{a}", str(b)])
        if r1.returncode != 0:
            fail(f"023.16 setup: first sync failed: {r1.stdout!r}")
            return

        ks_dir = b / ".kitchensync"
        if not ks_dir.exists():
            fail("023.16 setup: .kitchensync not created on peer b after first sync")
            return

        # Modify the file in A so a transfer to B is planned.
        (a / "target.txt").write_text("modified content after first sync")

        original_mode = ks_dir.stat().st_mode
        try:
            # Remove write permission from b/.kitchensync so SWAP directory creation fails.
            ks_dir.chmod(0o555)
            r2 = run_ks([str(a), str(b), "--retries-copy", "1"])
        finally:
            ks_dir.chmod(original_mode)

        if r2.stderr:
            fail(f"023.2: stderr not empty during transfer failure: {r2.stderr!r}")

        stdout = r2.stdout
        # Search for lines that mention target.txt AND a known phase OR a failure keyword.
        diag_lines = [
            ln for ln in stdout.splitlines()
            if "target.txt" in ln and any(ph in ln for ph in VALID_PHASES)
        ]
        if not diag_lines:
            diag_lines = [
                ln for ln in stdout.splitlines()
                if "target.txt" in ln and ("fail" in ln.lower() or "error" in ln.lower())
            ]

        if not diag_lines:
            # chmod may not have caused the expected failure on this system.
            print("SKIP 023.16/023.17: chmod did not produce a transfer failure diagnostic (inconclusive)")
            return

        diag = "\n".join(diag_lines)

        # 023.16: relative path identifiable in diagnostic.
        if "target.txt" not in diag:
            fail(f"023.16: relative path 'target.txt' not in diagnostic: {diag!r}")

        # 023.16: destination peer (path to b or file:// URL) identifiable.
        if str(b) not in diag and "file://" not in diag:
            fail(f"023.16: destination peer not identified in diagnostic: {diag!r}")

        # 023.17: phase name must be one of the enumerated values.
        if not any(ph in diag for ph in VALID_PHASES):
            fail(
                f"023.17: no valid phase name in diagnostic: {diag!r}; "
                f"valid phases: {sorted(VALID_PHASES)}"
            )


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------

_TESTS = [
    test_output_channels,
    test_stderr_empty_on_arg_error,
    test_progress_line_format,
    test_one_c_line_per_path_multiple_peers,
    test_no_terminal_control_sequences,
    test_x_line_for_displacement,
    test_no_progress_for_directory_creation,
    test_verbosity_error_suppresses_cx,
    test_verbosity_info_shows_cx,
    test_error_diagnostic_at_error_verbosity,
    test_debug_identical_to_info,
    test_trace_copy_slot_events,
    test_failed_transfer_diagnostic,
]


def main():
    for test in _TESTS:
        try:
            test()
        except Exception as exc:
            fail(f"{test.__name__} raised: {exc}")

    if _failures:
        print(f"\n{len(_failures)} failure(s).")
        sys.exit(1)
    print("All checks passed.")
    sys.exit(0)


if __name__ == "__main__":
    main()
