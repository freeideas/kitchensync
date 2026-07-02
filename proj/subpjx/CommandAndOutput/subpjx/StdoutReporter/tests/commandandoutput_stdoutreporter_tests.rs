use std::env;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use commandandoutput_stdoutreporter::{
    new, StdoutErrorDiagnostic, StdoutErrorKind, StdoutFailedFileTransferDiagnostic,
    StdoutFileTransferPhase, StdoutVerbosity,
};

const HELPER_ENV: &str = "KITCHENSYNC_STDOUTREPORTER_HELPER";
const START_MARKER: &str = "STDOUTREPORTER_CAPTURE_START";
const END_MARKER: &str = "STDOUTREPORTER_CAPTURE_END";

fn run_helper(mode: &str) -> String {
    let exe = env::current_exe().expect("current test executable path");
    let mut child = Command::new(exe)
        .arg("--exact")
        .arg("stdoutreporter_child_helper")
        .arg("--nocapture")
        .env(HELPER_ENV, mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stdout reporter helper");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().expect("poll stdout reporter helper").is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill timed out stdout reporter helper");
            let output = child
                .wait_with_output()
                .expect("collect timed out stdout reporter helper output");
            panic!(
                "stdout reporter helper timed out; stdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }

    let output = child
        .wait_with_output()
        .expect("collect stdout reporter helper output");
    assert!(
        output.status.success(),
        "stdout reporter helper failed; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).expect("helper stderr is utf-8");
    assert_eq!("", stderr, "StdoutReporter must leave stderr empty");

    let stdout = String::from_utf8(output.stdout).expect("helper stdout is utf-8");
    let start = stdout
        .find(START_MARKER)
        .expect("helper stdout contains start marker")
        + START_MARKER.len();
    let after_start = stdout[start..].strip_prefix('\n').unwrap_or(&stdout[start..]);
    let end = after_start
        .find(END_MARKER)
        .expect("helper stdout contains end marker");

    after_start[..end].to_string()
}

fn assert_plain_lines(output: &str) {
    assert!(!output.contains('\r'), "output must not use carriage returns");
    assert!(!output.contains('\x1b'), "output must not use escape sequences");
    for byte in output.bytes() {
        assert!(
            byte == b'\n' || byte >= b' ',
            "output must not contain terminal control bytes"
        );
    }
}

#[test]
fn argument_failure_writes_error_then_help_to_stdout_at_every_verbosity() {
    let output = run_helper("argument_failure_all_verbosities");

    assert_eq!(
        concat!(
            "invalid peer argument\n",
            "Usage: kitchensync [OPTIONS] <PEERS>\n",
            "invalid peer argument\n",
            "Usage: kitchensync [OPTIONS] <PEERS>\n",
            "invalid peer argument\n",
            "Usage: kitchensync [OPTIONS] <PEERS>\n",
            "invalid peer argument\n",
            "Usage: kitchensync [OPTIONS] <PEERS>\n",
        ),
        output
    );
    assert_plain_lines(&output);
}

#[test]
fn decision_failures_error_diagnostics_and_completion_are_stdout_lines() {
    let output = run_helper("error_diagnostics_and_completion");
    let lines: Vec<&str> = output.lines().collect();

    assert_eq!(
        "First sync? Mark the authoritative peer with a leading +",
        lines[0]
    );
    assert_eq!(
        "No contributing peer reachable - cannot make sync decisions",
        lines[1]
    );
    assert_eq!("sync complete", lines[lines.len() - 1]);

    let diagnostic_details = [
        "detail-argument",
        "detail-no-snapshots",
        "detail-unreachable-peer",
        "detail-directory-listing",
        "detail-canon-peer",
        "detail-fewer-than-two",
        "detail-no-contributing",
        "detail-transfer-before",
        "detail-transfer-after",
        "detail-archive-old",
        "detail-displacement",
        "detail-staging",
        "detail-set-mod-time",
        "detail-snapshot-before",
        "detail-snapshot-after",
    ];
    for detail in diagnostic_details {
        assert!(
            lines.iter().any(|line| line.contains(detail)),
            "missing diagnostic detail {detail}"
        );
    }
    assert_plain_lines(&output);
}

#[test]
fn progress_lines_are_info_level_plain_lines_in_reported_order() {
    let output = run_helper("progress_order_and_filtering");

    assert_eq!(
        concat!(
            "C alpha/file.txt\n",
            "X beta/displaced.txt\n",
            "C gamma/debug.txt\n",
            "X delta/debug-dir\n",
            "C epsilon/trace.txt\n",
            "X zeta/trace-dir\n",
            "copy-slots active=2/4\n",
        ),
        output
    );
    assert_plain_lines(&output);
}

#[test]
fn debug_verbosity_has_same_observable_output_as_info() {
    let info_output = run_helper("observable_info");
    let debug_output = run_helper("observable_debug");

    assert_eq!(info_output, debug_output);
    assert_eq!("C same/file.txt\nX same/displaced\nfinished\n", info_output);
}

#[test]
fn failed_file_transfer_diagnostics_include_required_fields() {
    let output = run_helper("failed_file_transfers");
    let expected_phases = [
        "read_source",
        "write_swap_new",
        "move_existing_to_swap_old",
        "rename_final",
        "set_mod_time",
        "archive_old",
        "cleanup",
    ];
    let lines: Vec<&str> = output.lines().collect();

    assert_eq!(expected_phases.len(), lines.len());
    for (line, phase) in lines.iter().zip(expected_phases) {
        assert!(
            line.contains("dir/subdir/file.txt"),
            "diagnostic must identify the slash-separated relative path"
        );
        assert!(
            line.contains("sftp://peer.example/sync-root"),
            "diagnostic must identify the destination peer URL"
        );
        assert!(
            line.contains(phase),
            "diagnostic must identify failed phase label {phase}"
        );
        assert!(
            line.contains("io-timeout"),
            "diagnostic must identify available transport error category"
        );
    }
    assert_plain_lines(&output);
}

#[test]
fn stdoutreporter_child_helper() {
    let Ok(mode) = env::var(HELPER_ENV) else {
        return;
    };

    println!("{START_MARKER}");
    match mode.as_str() {
        "argument_failure_all_verbosities" => emit_argument_failure_all_verbosities(),
        "error_diagnostics_and_completion" => emit_error_diagnostics_and_completion(),
        "progress_order_and_filtering" => emit_progress_order_and_filtering(),
        "observable_info" => emit_observable(StdoutVerbosity::Info),
        "observable_debug" => emit_observable(StdoutVerbosity::Debug),
        "failed_file_transfers" => emit_failed_file_transfers(),
        _ => panic!("unknown stdout reporter helper mode {mode}"),
    }
    println!("{END_MARKER}");
}

fn emit_argument_failure_all_verbosities() {
    let reporter = new();
    for verbosity in [
        StdoutVerbosity::Error,
        StdoutVerbosity::Info,
        StdoutVerbosity::Debug,
        StdoutVerbosity::Trace,
    ] {
        reporter.report_argument_validation_failure(
            verbosity,
            "invalid peer argument".to_string(),
            "Usage: kitchensync [OPTIONS] <PEERS>\n".to_string(),
        );
    }
}

fn emit_error_diagnostics_and_completion() {
    let reporter = new();
    reporter.report_first_sync_requires_authoritative_peer(StdoutVerbosity::Error);
    reporter.report_no_contributing_peer_reachable(StdoutVerbosity::Error);

    for (kind, details) in [
        (StdoutErrorKind::ArgumentError, "detail-argument"),
        (
            StdoutErrorKind::NoSnapshotsAndNoCanon,
            "detail-no-snapshots",
        ),
        (StdoutErrorKind::UnreachablePeer, "detail-unreachable-peer"),
        (
            StdoutErrorKind::DirectoryListingFailure,
            "detail-directory-listing",
        ),
        (StdoutErrorKind::CanonPeerUnreachable, "detail-canon-peer"),
        (
            StdoutErrorKind::FewerThanTwoReachablePeers,
            "detail-fewer-than-two",
        ),
        (
            StdoutErrorKind::NoContributingPeerReachable,
            "detail-no-contributing",
        ),
        (
            StdoutErrorKind::TransferFailureBeforeSwapOld,
            "detail-transfer-before",
        ),
        (
            StdoutErrorKind::TransferFailureAfterSwapOld,
            "detail-transfer-after",
        ),
        (StdoutErrorKind::ArchiveOldFailure, "detail-archive-old"),
        (StdoutErrorKind::DisplacementFailure, "detail-displacement"),
        (
            StdoutErrorKind::TmpOrSwapStagingFailure,
            "detail-staging",
        ),
        (StdoutErrorKind::SetModTimeFailure, "detail-set-mod-time"),
        (
            StdoutErrorKind::SnapshotUploadFailureBeforeSwapOld,
            "detail-snapshot-before",
        ),
        (
            StdoutErrorKind::SnapshotUploadFailureAfterSwapOld,
            "detail-snapshot-after",
        ),
    ] {
        reporter.report_error_diagnostic(
            StdoutVerbosity::Error,
            StdoutErrorDiagnostic {
                kind,
                details: details.to_string(),
            },
        );
    }

    reporter.report_completion(StdoutVerbosity::Error, "sync complete".to_string());
}

fn emit_progress_order_and_filtering() {
    let reporter = new();
    reporter.report_copy_progress(StdoutVerbosity::Error, "suppressed/copy.txt".to_string());
    reporter.report_displacement_progress(
        StdoutVerbosity::Error,
        "suppressed/displaced.txt".to_string(),
    );
    reporter.report_copy_progress(StdoutVerbosity::Info, "alpha/file.txt".to_string());
    reporter.report_displacement_progress(StdoutVerbosity::Info, "beta/displaced.txt".to_string());
    reporter.report_copy_progress(StdoutVerbosity::Debug, "gamma/debug.txt".to_string());
    reporter.report_displacement_progress(StdoutVerbosity::Debug, "delta/debug-dir".to_string());
    reporter.report_copy_progress(StdoutVerbosity::Trace, "epsilon/trace.txt".to_string());
    reporter.report_displacement_progress(StdoutVerbosity::Trace, "zeta/trace-dir".to_string());
    reporter.report_copy_slots(StdoutVerbosity::Info, 1, 4);
    reporter.report_copy_slots(StdoutVerbosity::Debug, 1, 4);
    reporter.report_copy_slots(StdoutVerbosity::Trace, 2, 4);
}

fn emit_observable(verbosity: StdoutVerbosity) {
    let reporter = new();
    reporter.report_copy_progress(verbosity, "same/file.txt".to_string());
    reporter.report_displacement_progress(verbosity, "same/displaced".to_string());
    reporter.report_copy_slots(verbosity, 3, 8);
    reporter.report_completion(verbosity, "finished".to_string());
}

fn emit_failed_file_transfers() {
    let reporter = new();
    for phase in [
        StdoutFileTransferPhase::ReadSource,
        StdoutFileTransferPhase::WriteSwapNew,
        StdoutFileTransferPhase::MoveExistingToSwapOld,
        StdoutFileTransferPhase::RenameFinal,
        StdoutFileTransferPhase::SetModTime,
        StdoutFileTransferPhase::ArchiveOld,
        StdoutFileTransferPhase::Cleanup,
    ] {
        reporter.report_failed_file_transfer(
            StdoutVerbosity::Error,
            StdoutFailedFileTransferDiagnostic {
                relpath: "dir/subdir/file.txt".to_string(),
                destination_peer_url: "sftp://peer.example/sync-root".to_string(),
                phase,
                transport_error_category: Some("io-timeout".to_string()),
            },
        );
    }
}
