use output::{FailedPhase, Output, Verbosity};
use std::process::Command;

// The Rust test harness captures stdout in-process via a thread-local
// intercept, so println! output is never visible to assertions made within
// the same process.  To observe what an Output impl actually prints, each
// test spawns the test binary as a subprocess with --nocapture (which
// disables the thread-local intercept) and captures the subprocess's fd-1
// with Command::output().  The env var OUTPUT_ISOLATED tells the child
// process to skip the subprocess-spawn path and run the real action.

fn is_isolated() -> bool {
    std::env::var("OUTPUT_ISOLATED").is_ok()
}

/// Run the named test in an isolated subprocess and return (stdout, stderr).
/// Panics if the subprocess exits with a failure status.
fn run_isolated(test_name: &str) -> (String, String) {
    let exe = std::env::current_exe().expect("cannot locate test binary");
    let proc = Command::new(&exe)
        .arg("--nocapture")
        .arg("--test-threads")
        .arg("1")
        .arg(test_name)
        .env("OUTPUT_ISOLATED", "1")
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn '{}': {}", test_name, e));
    assert!(
        proc.status.success(),
        "isolated test '{}' failed\nstdout:\n{}\nstderr:\n{}",
        test_name,
        String::from_utf8_lossy(&proc.stdout),
        String::from_utf8_lossy(&proc.stderr),
    );
    (
        String::from_utf8_lossy(&proc.stdout).into_owned(),
        String::from_utf8_lossy(&proc.stderr).into_owned(),
    )
}

// ─── 023.6 / 023.7: copied emits exactly "C <relpath>" ─────────────────────

#[test]
fn copied_emits_c_letter_space_relpath() {
    if !is_isolated() {
        let (out, _) = run_isolated("copied_emits_c_letter_space_relpath");
        assert!(
            out.contains("C docs/readme.md\n"),
            "expected 'C docs/readme.md' in:\n{}",
            out
        );
        return;
    }
    let subject = output::new();
    // default verbosity is Info; no set_verbosity call needed
    subject.copied("docs/readme.md");
}

// ─── 023.6 / 023.8: displaced emits exactly "X <relpath>" ──────────────────

#[test]
fn displaced_emits_x_letter_space_relpath() {
    if !is_isolated() {
        let (out, _) = run_isolated("displaced_emits_x_letter_space_relpath");
        assert!(
            out.contains("X old/config.json\n"),
            "expected 'X old/config.json' in:\n{}",
            out
        );
        return;
    }
    let subject = output::new();
    subject.displaced("old/config.json");
}

// ─── 023.12: C/X lines suppressed at error verbosity ───────────────────────

#[test]
fn copied_suppressed_at_error_verbosity() {
    if !is_isolated() {
        let (out, _) = run_isolated("copied_suppressed_at_error_verbosity");
        // "\nC " would match a progress line; test harness lines never start with "C "
        assert!(
            !out.contains("\nC "),
            "unexpected C line at error verbosity:\n{}",
            out
        );
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Error);
    subject.copied("images/photo.jpg");
}

#[test]
fn displaced_suppressed_at_error_verbosity() {
    if !is_isolated() {
        let (out, _) = run_isolated("displaced_suppressed_at_error_verbosity");
        assert!(
            !out.contains("\nX "),
            "unexpected X line at error verbosity:\n{}",
            out
        );
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Error);
    subject.displaced("images/photo.jpg");
}

// ─── 023.11: diagnostic emits at error level ────────────────────────────────

#[test]
fn diagnostic_emits_at_error_verbosity() {
    if !is_isolated() {
        let (out, _) = run_isolated("diagnostic_emits_at_error_verbosity");
        assert!(
            out.contains("peer unreachable: sftp://host"),
            "expected diagnostic text in:\n{}",
            out
        );
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Error);
    subject.diagnostic("peer unreachable: sftp://host");
}

// ─── 023.13: debug verbosity is observationally identical to info ────────────

#[test]
fn debug_verbosity_emits_progress_lines_same_as_info() {
    if !is_isolated() {
        let (out, _) = run_isolated("debug_verbosity_emits_progress_lines_same_as_info");
        assert!(
            out.contains("C archive/notes.txt\n"),
            "expected C line at debug verbosity in:\n{}",
            out
        );
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Debug);
    subject.copied("archive/notes.txt");
}

// ─── 023.14 / 023.15: copy_slots emits "copy-slots active=N/M" at trace ────

#[test]
fn copy_slots_format_at_trace() {
    if !is_isolated() {
        let (out, _) = run_isolated("copy_slots_format_at_trace");
        assert!(
            out.contains("copy-slots active=3/8"),
            "expected 'copy-slots active=3/8' in:\n{}",
            out
        );
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Trace);
    subject.copy_slots(3, 8);
}

#[test]
fn copy_slots_suppressed_below_trace() {
    if !is_isolated() {
        let (out, _) = run_isolated("copy_slots_suppressed_below_trace");
        assert!(
            !out.contains("copy-slots"),
            "unexpected copy-slots line at info verbosity:\n{}",
            out
        );
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Info);
    subject.copy_slots(2, 4);
}

// ─── 023.10: trace emits progress lines AND slot events ─────────────────────

#[test]
fn trace_verbosity_emits_progress_and_slot_events() {
    if !is_isolated() {
        let (out, _) = run_isolated("trace_verbosity_emits_progress_and_slot_events");
        assert!(
            out.contains("C docs/report.pdf\n"),
            "C line missing at trace verbosity:\n{}",
            out
        );
        assert!(
            out.contains("copy-slots active=1/4"),
            "slot line missing at trace verbosity:\n{}",
            out
        );
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Trace);
    subject.copied("docs/report.pdf");
    subject.copy_slots(1, 4);
}

// ─── 023.16: transfer_failed identifies path, peer URL, and phase ────────────

#[test]
fn transfer_failed_contains_relpath_peer_url_phase_without_category() {
    if !is_isolated() {
        let (out, _) =
            run_isolated("transfer_failed_contains_relpath_peer_url_phase_without_category");
        assert!(out.contains("data/file.bin"), "relpath missing:\n{}", out);
        assert!(out.contains("sftp://peer1.example"), "peer_url missing:\n{}", out);
        assert!(out.contains("read_source"), "phase missing:\n{}", out);
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Error);
    subject.transfer_failed(
        "data/file.bin",
        "sftp://peer1.example",
        FailedPhase::ReadSource,
        None,
    );
}

#[test]
fn transfer_failed_contains_error_category_when_provided() {
    if !is_isolated() {
        let (out, _) = run_isolated("transfer_failed_contains_error_category_when_provided");
        assert!(out.contains("data/file.bin"), "relpath missing:\n{}", out);
        assert!(out.contains("sftp://peer2.example"), "peer_url missing:\n{}", out);
        assert!(out.contains("rename_final"), "phase missing:\n{}", out);
        assert!(out.contains("permission_denied"), "error_category missing:\n{}", out);
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Error);
    subject.transfer_failed(
        "data/file.bin",
        "sftp://peer2.example",
        FailedPhase::RenameFinal,
        Some("permission_denied"),
    );
}

// ─── 023.17: all seven failed-phase name strings ────────────────────────────

#[test]
fn transfer_failed_all_phase_names_match_spec() {
    if !is_isolated() {
        let (out, _) = run_isolated("transfer_failed_all_phase_names_match_spec");
        for phase in &[
            "read_source",
            "write_swap_new",
            "move_existing_to_swap_old",
            "rename_final",
            "set_mod_time",
            "archive_old",
            "cleanup",
        ] {
            assert!(
                out.contains(phase),
                "phase name '{}' missing from:\n{}",
                phase,
                out
            );
        }
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Error);
    subject.transfer_failed("f", "sftp://h", FailedPhase::ReadSource, None);
    subject.transfer_failed("f", "sftp://h", FailedPhase::WriteSwapNew, None);
    subject.transfer_failed("f", "sftp://h", FailedPhase::MoveExistingToSwapOld, None);
    subject.transfer_failed("f", "sftp://h", FailedPhase::RenameFinal, None);
    subject.transfer_failed("f", "sftp://h", FailedPhase::SetModTime, None);
    subject.transfer_failed("f", "sftp://h", FailedPhase::ArchiveOld, None);
    subject.transfer_failed("f", "sftp://h", FailedPhase::Cleanup, None);
}

// ─── 023.2: stderr remains empty ────────────────────────────────────────────

#[test]
fn stderr_stays_empty_during_all_operations() {
    if !is_isolated() {
        let (_, stderr) = run_isolated("stderr_stays_empty_during_all_operations");
        assert!(
            stderr.is_empty(),
            "expected empty stderr, got:\n{}",
            stderr
        );
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Trace);
    subject.copied("a/b.txt");
    subject.displaced("c/d.txt");
    subject.diagnostic("some error message");
    subject.transfer_failed("x.bin", "sftp://h", FailedPhase::Cleanup, Some("timeout"));
    subject.copy_slots(0, 2);
}

// ─── 023.3: progress lines appear in emission order ─────────────────────────

#[test]
fn progress_lines_appear_in_emission_order() {
    if !is_isolated() {
        let (out, _) = run_isolated("progress_lines_appear_in_emission_order");
        let c_pos = out
            .find("C first.txt\n")
            .expect("C first.txt not found in output");
        let x_pos = out
            .find("X second.txt\n")
            .expect("X second.txt not found in output");
        assert!(
            c_pos < x_pos,
            "C line should precede X line; positions {} vs {} in:\n{}",
            c_pos,
            x_pos,
            out
        );
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Info);
    subject.copied("first.txt");
    subject.displaced("second.txt");
}

// ─── 023.5: no terminal control sequences ───────────────────────────────────

#[test]
fn progress_output_contains_no_control_sequences() {
    if !is_isolated() {
        let (out, _) = run_isolated("progress_output_contains_no_control_sequences");
        for line in out.lines() {
            if line.starts_with("C ") || line.starts_with("X ") || line.starts_with("copy-slots")
            {
                assert!(
                    !line.contains('\x1b'),
                    "ANSI escape sequence in progress line: {:?}",
                    line
                );
                assert!(
                    line.bytes().all(|b| b >= 0x20 || b == b'\t'),
                    "control character in progress line: {:?}",
                    line
                );
            }
        }
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Trace);
    subject.copied("dir/file.txt");
    subject.displaced("dir/old.txt");
    subject.copy_slots(1, 2);
}

// ─── 024.9: dry-run emits same C/X progress lines as a normal run ────────────

#[test]
fn dry_run_emits_same_cx_progress_lines() {
    if !is_isolated() {
        let (out, _) = run_isolated("dry_run_emits_same_cx_progress_lines");
        assert!(
            out.contains("C sync/notes.txt\n"),
            "expected 'C sync/notes.txt' in dry-run output:\n{}",
            out
        );
        assert!(
            out.contains("X sync/old.txt\n"),
            "expected 'X sync/old.txt' in dry-run output:\n{}",
            out
        );
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Info);
    subject.copied("sync/notes.txt");
    subject.displaced("sync/old.txt");
}

// ─── 024.10: dry-run prints the "dry run" phrase on stdout ───────────────────

#[test]
fn dry_run_phrase_reaches_stdout() {
    if !is_isolated() {
        let (out, _) = run_isolated("dry_run_phrase_reaches_stdout");
        assert!(
            out.contains("dry run"),
            "expected 'dry run' phrase on stdout:\n{}",
            out
        );
        return;
    }
    let subject = output::new();
    subject.set_verbosity(Verbosity::Info);
    subject.diagnostic("dry run: no changes were written");
}
