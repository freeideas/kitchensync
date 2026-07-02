use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use commandandoutput::{
    CommandAndOutput, CommandParseResult, FileTransferPhase, LocalPeerTarget, OutputEvent,
    PeerLocation, PeerRole, SftpPeerTarget, SyncErrorDiagnostic, SyncErrorKind,
    UrlConnectionSettings, Verbosity,
};

fn subject() -> Arc<dyn CommandAndOutput> {
    commandandoutput::new(
        commandandoutput_globalargumentparser::new(),
        commandandoutput_peerargumentparser::new(),
        commandandoutput_peeridentitynormalizer::new(),
        commandandoutput_stdoutreporter::new(),
    )
}

fn arg(value: &str) -> String {
    value.to_owned()
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| arg(value)).collect()
}

fn help_text() -> String {
    let source = include_str!("../../../../specs/help.md");
    let start = source.find("```\n").expect("help fence starts") + 4;
    let end = source[start..].find("\n```").expect("help fence ends") + start;
    source[start..end].to_owned()
}

fn parse(values: &[&str]) -> CommandParseResult {
    subject().parse_command(args(values), PathBuf::from("/work"), arg("alice"))
}

fn run(values: &[&str]) -> commandandoutput::RunRequest {
    let result = parse(values);
    let CommandParseResult::Run(run) = result else {
        panic!("expected run request for {values:?}");
    };
    run
}

fn validation_failure(values: &[&str]) -> commandandoutput::CommandProcessOutput {
    let result = parse(values);
    let CommandParseResult::ValidationFailure(output) = result else {
        panic!("expected validation failure for {values:?}");
    };
    output
}

fn captured_write_output(case_name: &str) -> String {
    let mut child = Command::new(std::env::current_exe().expect("current test executable"))
        .arg("--ignored")
        .arg("--exact")
        .arg("write_output_subprocess_helper")
        .arg("--nocapture")
        .env("COMMANDANDOUTPUT_CAPTURE_CASE", case_name)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("capture helper runs");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().expect("capture helper status").is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().expect("capture helper output");
            panic!(
                "capture helper timed out\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }

    let output = child.wait_with_output().expect("capture helper output");

    assert!(
        output.status.success(),
        "capture helper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stderr,
        Vec::<u8>::new(),
        "write_output wrote stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let start_marker = "__COMMANDANDOUTPUT_CAPTURE_BEGIN__\n";
    let end_marker = "__COMMANDANDOUTPUT_CAPTURE_END__";
    let start = stdout.find(start_marker).expect("capture starts") + start_marker.len();
    let end = stdout[start..].find(end_marker).expect("capture ends") + start;
    stdout[start..end]
        .strip_suffix('\n')
        .unwrap_or(&stdout[start..end])
        .to_owned()
}

#[test]
#[ignore]
fn write_output_subprocess_helper() {
    let Ok(case_name) = std::env::var("COMMANDANDOUTPUT_CAPTURE_CASE") else {
        return;
    };

    let command = subject();
    println!("__COMMANDANDOUTPUT_CAPTURE_BEGIN__");
    match case_name.as_str() {
        "error_filters_info_but_keeps_completion" => {
            command.write_output(
                Verbosity::Error,
                OutputEvent::CopyProgress {
                    relpath: arg("dir/file.txt"),
                },
            );
            command.write_output(
                Verbosity::Error,
                OutputEvent::DisplacementProgress {
                    relpath: arg("old/file.txt"),
                },
            );
            command.write_output(Verbosity::Error, OutputEvent::CopySlots { active: 1, max: 3 });
            command.write_output(Verbosity::Error, OutputEvent::Completion);
        }
        "info_output" => {
            command.write_output(
                Verbosity::Info,
                OutputEvent::CopyProgress {
                    relpath: arg("dir/file.txt"),
                },
            );
            command.write_output(
                Verbosity::Info,
                OutputEvent::DisplacementProgress {
                    relpath: arg("old/file.txt"),
                },
            );
            command.write_output(Verbosity::Info, OutputEvent::CopySlots { active: 1, max: 3 });
            command.write_output(Verbosity::Info, OutputEvent::Completion);
        }
        "debug_output" => {
            command.write_output(
                Verbosity::Debug,
                OutputEvent::CopyProgress {
                    relpath: arg("dir/file.txt"),
                },
            );
            command.write_output(
                Verbosity::Debug,
                OutputEvent::DisplacementProgress {
                    relpath: arg("old/file.txt"),
                },
            );
            command.write_output(Verbosity::Debug, OutputEvent::CopySlots { active: 1, max: 3 });
            command.write_output(Verbosity::Debug, OutputEvent::Completion);
        }
        "trace_output" => {
            command.write_output(
                Verbosity::Trace,
                OutputEvent::CopyProgress {
                    relpath: arg("dir/file.txt"),
                },
            );
            command.write_output(
                Verbosity::Trace,
                OutputEvent::DisplacementProgress {
                    relpath: arg("old/file.txt"),
                },
            );
            command.write_output(Verbosity::Trace, OutputEvent::CopySlots { active: 1, max: 3 });
            command.write_output(Verbosity::Trace, OutputEvent::Completion);
        }
        "diagnostics" => {
            command.write_output(Verbosity::Error, OutputEvent::FirstSyncNeedsCanon);
            command.write_output(Verbosity::Error, OutputEvent::NoContributingPeerReachable);
            command.write_output(
                Verbosity::Error,
                OutputEvent::ErrorDiagnostic(SyncErrorDiagnostic {
                    kind: SyncErrorKind::DirectoryListingFailure,
                    details: arg("listing failed for sftp://host.example/root"),
                }),
            );
            command.write_output(
                Verbosity::Error,
                OutputEvent::FailedFileTransfer(commandandoutput::FailedFileTransferDiagnostic {
                    relpath: arg("dir/file.txt"),
                    destination_peer_url: arg("sftp://host.example/root"),
                    phase: FileTransferPhase::WriteSwapNew,
                    transport_error_category: Some(arg("permission_denied")),
                }),
            );
        }
        "argument_validation_failure" => {
            command.write_output(
                Verbosity::Error,
                OutputEvent::ArgumentValidationFailure(
                    commandandoutput::ArgumentValidationFailureOutput {
                        error_message: arg("bad arguments"),
                        help_text: help_text(),
                    },
                ),
            );
        }
        "all_error_diagnostics" => {
            for (kind, details) in [
                (
                    SyncErrorKind::NoSnapshotsAndNoCanon,
                    "detail-no-snapshots-and-no-canon",
                ),
                (SyncErrorKind::UnreachablePeer, "detail-unreachable-peer"),
                (
                    SyncErrorKind::DirectoryListingFailure,
                    "detail-directory-listing-failure",
                ),
                (
                    SyncErrorKind::CanonPeerUnreachable,
                    "detail-canon-peer-unreachable",
                ),
                (
                    SyncErrorKind::FewerThanTwoReachablePeers,
                    "detail-fewer-than-two-reachable-peers",
                ),
                (
                    SyncErrorKind::NoContributingPeerReachable,
                    "detail-no-contributing-peer-reachable",
                ),
                (
                    SyncErrorKind::TransferFailureBeforeSwapOld,
                    "detail-transfer-failure-before-swap-old",
                ),
                (
                    SyncErrorKind::TransferFailureAfterSwapOld,
                    "detail-transfer-failure-after-swap-old",
                ),
                (SyncErrorKind::ArchiveOldFailure, "detail-archive-old-failure"),
                (
                    SyncErrorKind::DisplacementFailure,
                    "detail-displacement-failure",
                ),
                (
                    SyncErrorKind::TmpOrSwapStagingFailure,
                    "detail-tmp-or-swap-staging-failure",
                ),
                (SyncErrorKind::SetModTimeFailure, "detail-set-mod-time-failure"),
                (
                    SyncErrorKind::SnapshotUploadFailureBeforeSwapOld,
                    "detail-snapshot-upload-failure-before-swap-old",
                ),
                (
                    SyncErrorKind::SnapshotUploadFailureAfterSwapOld,
                    "detail-snapshot-upload-failure-after-swap-old",
                ),
            ] {
                command.write_output(
                    Verbosity::Error,
                    OutputEvent::ErrorDiagnostic(SyncErrorDiagnostic {
                        kind,
                        details: arg(details),
                    }),
                );
            }
        }
        "all_transfer_phases" => {
            for phase in [
                FileTransferPhase::ReadSource,
                FileTransferPhase::WriteSwapNew,
                FileTransferPhase::MoveExistingToSwapOld,
                FileTransferPhase::RenameFinal,
                FileTransferPhase::SetModTime,
                FileTransferPhase::ArchiveOld,
                FileTransferPhase::Cleanup,
            ] {
                command.write_output(
                    Verbosity::Error,
                    OutputEvent::FailedFileTransfer(
                        commandandoutput::FailedFileTransferDiagnostic {
                            relpath: arg("dir/subdir/file.txt"),
                            destination_peer_url: arg("sftp://host.example/root"),
                            phase,
                            transport_error_category: Some(arg("io-timeout")),
                        },
                    ),
                );
            }
        }
        _ => panic!("unknown capture case {case_name}"),
    }
    println!("__COMMANDANDOUTPUT_CAPTURE_END__");
}

#[test]
fn no_arguments_return_verbatim_help_on_stdout() {
    let result = subject().parse_command(Vec::new(), PathBuf::from("/work"), arg("alice"));

    let CommandParseResult::Help(output) = result else {
        panic!("expected help result");
    };

    assert_eq!(output.stdout, help_text());
    assert_eq!(output.stderr, "");
    assert_eq!(output.exit_code, 0);
}

#[test]
fn write_output_filters_progress_by_verbosity_and_keeps_completion_visible() {
    assert_eq!(
        captured_write_output("error_filters_info_but_keeps_completion"),
        "sync complete"
    );

    let info = "C dir/file.txt\nX old/file.txt\nsync complete";
    assert_eq!(captured_write_output("info_output"), info);
    assert_eq!(captured_write_output("debug_output"), info);
    assert_eq!(
        captured_write_output("trace_output"),
        "C dir/file.txt\nX old/file.txt\ncopy-slots active=1/3\nsync complete"
    );
}

#[test]
fn write_output_emits_required_diagnostic_lines() {
    let output = captured_write_output("diagnostics");

    assert!(output.contains("First sync? Mark the authoritative peer with a leading +"));
    assert!(output.contains("No contributing peer reachable - cannot make sync decisions"));
    assert!(output.contains("listing failed for sftp://host.example/root"));
    assert!(output.contains("dir/file.txt"));
    assert!(output.contains("sftp://host.example/root"));
    assert!(output.contains("write_swap_new"));
    let lower_output = output.to_lowercase();
    assert!(lower_output.contains("permission"));
    assert!(lower_output.contains("denied"));
}

#[test]
fn write_output_emits_argument_validation_failure_as_error_then_help() {
    let output = captured_write_output("argument_validation_failure");

    assert_eq!(output, format!("bad arguments\n{}", help_text()));
}

#[test]
fn write_output_emits_each_sync_error_condition_as_a_diagnostic() {
    let output = captured_write_output("all_error_diagnostics");

    for detail in [
        "detail-no-snapshots-and-no-canon",
        "detail-unreachable-peer",
        "detail-directory-listing-failure",
        "detail-canon-peer-unreachable",
        "detail-fewer-than-two-reachable-peers",
        "detail-no-contributing-peer-reachable",
        "detail-transfer-failure-before-swap-old",
        "detail-transfer-failure-after-swap-old",
        "detail-archive-old-failure",
        "detail-displacement-failure",
        "detail-tmp-or-swap-staging-failure",
        "detail-set-mod-time-failure",
        "detail-snapshot-upload-failure-before-swap-old",
        "detail-snapshot-upload-failure-after-swap-old",
    ] {
        assert!(output.contains(detail), "missing diagnostic detail {detail}");
    }
}

#[test]
fn failed_file_transfer_diagnostics_cover_each_required_phase_label() {
    let output = captured_write_output("all_transfer_phases");
    let lines: Vec<&str> = output.lines().collect();
    let expected_phases = [
        "read_source",
        "write_swap_new",
        "move_existing_to_swap_old",
        "rename_final",
        "set_mod_time",
        "archive_old",
        "cleanup",
    ];

    assert_eq!(lines.len(), expected_phases.len());
    for (line, phase) in lines.iter().zip(expected_phases) {
        assert!(line.contains("dir/subdir/file.txt"));
        assert!(line.contains("sftp://host.example/root"));
        assert!(line.contains(phase));
        assert!(line.contains("io-timeout"));
    }
}

#[test]
fn valid_invocation_parses_globals_peers_fallbacks_and_normalized_identities() {
    let result = subject().parse_command(
        vec![
            arg("--dry-run"),
            arg("--max-copies"),
            arg("7"),
            arg("--retries-copy"),
            arg("4"),
            arg("--retries-list"),
            arg("5"),
            arg("--timeout-conn"),
            arg("40"),
            arg("--timeout-idle"),
            arg("50"),
            arg("--verbosity"),
            arg("trace"),
            arg("--keep-tmp-days"),
            arg("3"),
            arg("--keep-bak-days"),
            arg("91"),
            arg("--keep-del-days"),
            arg("181"),
            arg("-x"),
            arg("cache/tmp"),
            arg("-x"),
            arg("logs/archive"),
            arg("+[sftp://USER:p%40ss%3Aword@host.example:22/Root/Data?timeout-conn=11,sftp://backup.example:2200/root?timeout-idle=12]"),
            arg("-C:\\sync\\subordinate"),
            arg("relative/normal"),
            arg("file:///archive/local"),
        ],
        PathBuf::from("/current/root"),
        arg("localuser"),
    );

    let CommandParseResult::Run(run) = result else {
        panic!("expected run request");
    };

    assert_eq!(run.settings.dry_run, true);
    assert_eq!(run.settings.max_copies, 7);
    assert_eq!(run.settings.retries_copy, 4);
    assert_eq!(run.settings.retries_list, 5);
    assert_eq!(run.settings.timeout_conn_seconds, 40);
    assert_eq!(run.settings.timeout_idle_seconds, 50);
    assert_eq!(run.settings.verbosity, Verbosity::Trace);
    assert_eq!(run.settings.keep_tmp_days, 3);
    assert_eq!(run.settings.keep_bak_days, 91);
    assert_eq!(run.settings.keep_del_days, 181);
    assert_eq!(run.settings.excludes, vec!["cache/tmp", "logs/archive"]);

    assert_eq!(run.peers.len(), 4);
    assert_eq!(run.peers[0].role, PeerRole::Canon);
    assert_eq!(run.peers[1].role, PeerRole::Subordinate);
    assert_eq!(run.peers[2].role, PeerRole::Normal);
    assert_eq!(run.peers[3].role, PeerRole::Normal);

    assert_eq!(run.peers[0].fallback_targets.len(), 2);
    let first_target = &run.peers[0].fallback_targets[0];
    assert_eq!(
        first_target.connection,
        UrlConnectionSettings {
            timeout_conn_seconds: 11,
            timeout_idle_seconds: 50,
        }
    );
    assert_eq!(
        first_target.normalized_identity,
        "sftp://USER@host.example/Root/Data"
    );
    let PeerLocation::Sftp(first_sftp) = &first_target.location else {
        panic!("expected first fallback to be sftp");
    };
    assert_eq!(first_sftp.host, "host.example");
    assert_eq!(first_sftp.username, "USER");
    assert_eq!(first_sftp.username_was_explicit, true);
    assert_eq!(first_sftp.password.as_deref(), Some("p@ss:word"));
    assert_eq!(first_sftp.port, 22);
    assert_eq!(first_sftp.absolute_path, "/Root/Data");

    let second_target = &run.peers[0].fallback_targets[1];
    assert_eq!(
        second_target.connection,
        UrlConnectionSettings {
            timeout_conn_seconds: 40,
            timeout_idle_seconds: 12,
        }
    );
    assert_eq!(
        second_target.normalized_identity,
        "sftp://localuser@backup.example:2200/root"
    );
    let PeerLocation::Sftp(second_sftp) = &second_target.location else {
        panic!("expected second fallback to be sftp");
    };
    assert_eq!(second_sftp.username, "localuser");
    assert_eq!(second_sftp.username_was_explicit, false);
    assert_eq!(second_sftp.port, 2200);

    let PeerLocation::Local(_) = &run.peers[1].fallback_targets[0].location else {
        panic!("expected windows drive path to be local");
    };
    assert!(
        run.peers[1].fallback_targets[0]
            .normalized_identity
            .starts_with("file:///C:/sync/subordinate")
    );

    let PeerLocation::Local(_) = &run.peers[2].fallback_targets[0].location else {
        panic!("expected relative path to be local");
    };
    assert!(
        run.peers[2].fallback_targets[0]
            .normalized_identity
            .ends_with("/current/root/relative/normal")
    );

    let PeerLocation::Local(_) = &run.peers[3].fallback_targets[0].location else {
        panic!("expected file URL to be local");
    };
    assert_eq!(
        run.peers[3].fallback_targets[0].normalized_identity,
        "file:///archive/local"
    );
}

#[test]
fn defaults_are_applied_when_options_are_omitted() {
    let result = subject().parse_command(
        vec![arg("+/canon"), arg("sftp://example.test/root")],
        PathBuf::from("/work"),
        arg("alice"),
    );

    let CommandParseResult::Run(run) = result else {
        panic!("expected run request");
    };

    assert_eq!(run.settings.dry_run, false);
    assert_eq!(run.settings.max_copies, 10);
    assert_eq!(run.settings.retries_copy, 3);
    assert_eq!(run.settings.retries_list, 3);
    assert_eq!(run.settings.timeout_conn_seconds, 30);
    assert_eq!(run.settings.timeout_idle_seconds, 30);
    assert_eq!(run.settings.verbosity, Verbosity::Info);
    assert_eq!(run.settings.keep_tmp_days, 2);
    assert_eq!(run.settings.keep_bak_days, 90);
    assert_eq!(run.settings.keep_del_days, 180);
    assert_eq!(run.settings.excludes, Vec::<String>::new());
}

#[test]
fn validation_failures_return_error_help_exit_one_and_empty_stderr() {
    let cases: &[&[&str]] = &[
        &["/only-one-peer"],
        &["+/one", "+/two"],
        &["--unknown", "/one", "/two"],
        &["--max-copies", "0", "/one", "/two"],
        &["--verbosity", "verbose", "/one", "/two"],
        &["-x", "../bad", "/one", "/two"],
        &["sftp://host/root?max-copies=2", "/two"],
        &["sftp://host/root?unexpected=1", "/two"],
        &["sftp://host/root?timeout-conn=zero", "/two"],
        &["--timeout-idle"],
    ];

    for args in cases {
        let output = validation_failure(args);

        assert!(output.stdout.ends_with(&help_text()));
        assert_ne!(output.stdout, help_text());
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 1);
    }
}

#[test]
fn each_positive_integer_global_option_is_accepted_and_recorded() {
    let cases: &[(&[&str], fn(&commandandoutput::RunSettings) -> u32, u32)] = &[
        (&["--max-copies", "1"], |settings| settings.max_copies, 1),
        (
            &["--retries-copy", "2"],
            |settings| settings.retries_copy,
            2,
        ),
        (
            &["--retries-list", "4"],
            |settings| settings.retries_list,
            4,
        ),
        (
            &["--timeout-conn", "5"],
            |settings| settings.timeout_conn_seconds,
            5,
        ),
        (
            &["--timeout-idle", "6"],
            |settings| settings.timeout_idle_seconds,
            6,
        ),
        (
            &["--keep-tmp-days", "7"],
            |settings| settings.keep_tmp_days,
            7,
        ),
        (
            &["--keep-bak-days", "8"],
            |settings| settings.keep_bak_days,
            8,
        ),
        (
            &["--keep-del-days", "9"],
            |settings| settings.keep_del_days,
            9,
        ),
    ];

    for (option_args, field, expected) in cases {
        let mut invocation = option_args.to_vec();
        invocation.extend(["/left", "/right"]);

        assert_eq!(field(&run(&invocation).settings), *expected);
    }
}

#[test]
fn each_positive_integer_global_option_rejects_non_positive_or_non_integer_values() {
    let cases: &[(&str, &str)] = &[
        ("--max-copies", "0"),
        ("--retries-copy", "-1"),
        ("--retries-list", "1.5"),
        ("--timeout-conn", "many"),
        ("--timeout-idle", ""),
        ("--keep-tmp-days", "0"),
        ("--keep-bak-days", "none"),
        ("--keep-del-days", "-7"),
    ];

    for (option, value) in cases {
        let output = validation_failure(&[option, value, "/left", "/right"]);

        assert!(output.stdout.ends_with(&help_text()));
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 1);
    }
}

#[test]
fn each_valued_global_option_rejects_a_missing_value() {
    for option in [
        "--max-copies",
        "--retries-copy",
        "--retries-list",
        "--timeout-conn",
        "--timeout-idle",
        "--keep-tmp-days",
        "--keep-bak-days",
        "--keep-del-days",
        "--verbosity",
        "-x",
    ] {
        let output = validation_failure(&[option]);

        assert!(output.stdout.ends_with(&help_text()));
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 1);
    }
}

#[test]
fn accepted_verbosity_values_are_recorded() {
    let cases = [
        ("error", Verbosity::Error),
        ("info", Verbosity::Info),
        ("debug", Verbosity::Debug),
        ("trace", Verbosity::Trace),
    ];

    for (value, expected) in cases {
        let run = run(&["--verbosity", value, "/left", "/right"]);

        assert_eq!(run.settings.verbosity, expected);
    }
}

#[test]
fn exclude_paths_accept_repetition_and_reject_each_invalid_shape() {
    let run = run(&[
        "-x",
        "cache/tmp",
        "-x",
        "logs/archive",
        "/left",
        "/right",
    ]);
    assert_eq!(run.settings.excludes, vec!["cache/tmp", "logs/archive"]);

    let cases = [
        "/absolute",
        "trailing/",
        "has\\backslash",
        "empty//segment",
        "./dot",
        "parent/..",
        "has\0nul",
    ];

    for value in cases {
        let output = validation_failure(&["-x", value, "/left", "/right"]);

        assert!(output.stdout.ends_with(&help_text()));
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 1);
    }
}

#[test]
fn url_timeout_query_values_are_per_target_and_override_globals() {
    let run = run(&[
        "--timeout-conn",
        "40",
        "--timeout-idle",
        "50",
        "sftp://host.example/root?timeout-conn=11&timeout-idle=12",
        "sftp://host.example/other",
    ]);

    assert_eq!(
        run.peers[0].fallback_targets[0].connection,
        UrlConnectionSettings {
            timeout_conn_seconds: 11,
            timeout_idle_seconds: 12,
        }
    );
    assert_eq!(
        run.peers[1].fallback_targets[0].connection,
        UrlConnectionSettings {
            timeout_conn_seconds: 40,
            timeout_idle_seconds: 50,
        }
    );
}

#[test]
fn url_query_validation_rejects_unsupported_or_invalid_timeout_parameters() {
    let cases = [
        "sftp://host/root?max-copies=2",
        "sftp://host/root?unexpected=1",
        "sftp://host/root?timeout-conn=0",
        "sftp://host/root?timeout-conn=abc",
        "sftp://host/root?timeout-idle=0",
        "sftp://host/root?timeout-idle=abc",
    ];

    for peer in cases {
        let output = validation_failure(&[peer, "/right"]);

        assert!(output.stdout.ends_with(&help_text()));
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 1);
    }
}

#[test]
fn peer_roles_and_fallback_locations_preserve_command_order() {
    let run = run(&[
        "+[sftp://first.example/root,/local/fallback]",
        "-/subordinate",
        "-sftp://second.example/root",
        "/normal",
    ]);

    assert_eq!(run.peers.len(), 4);
    assert_eq!(run.peers[0].role, PeerRole::Canon);
    assert_eq!(run.peers[1].role, PeerRole::Subordinate);
    assert_eq!(run.peers[2].role, PeerRole::Subordinate);
    assert_eq!(run.peers[3].role, PeerRole::Normal);
    assert_eq!(run.peers[0].fallback_targets.len(), 2);

    let PeerLocation::Sftp(first) = &run.peers[0].fallback_targets[0].location else {
        panic!("expected first fallback to be sftp");
    };
    assert_eq!(first.host, "first.example");

    let PeerLocation::Local(second) = &run.peers[0].fallback_targets[1].location else {
        panic!("expected second fallback to be local");
    };
    assert_eq!(second.path_or_url, "/local/fallback");
}

#[test]
fn command_options_are_consumed_before_peer_operands() {
    let run = run(&["--dry-run", "--max-copies", "2", "/left", "/right", "/third"]);

    assert_eq!(run.settings.dry_run, true);
    assert_eq!(run.settings.max_copies, 2);
    assert_eq!(run.peers.len(), 3);
    assert_eq!(run.peers[0].fallback_targets[0].normalized_identity, "file:///left");
    assert_eq!(
        run.peers[1].fallback_targets[0].normalized_identity,
        "file:///right"
    );
    assert_eq!(
        run.peers[2].fallback_targets[0].normalized_identity,
        "file:///third"
    );
}

#[test]
fn normalizes_peer_identities_from_public_peer_locations() {
    let command = subject();

    let relative = command
        .normalize_peer_identity(
            PeerLocation::Local(LocalPeerTarget {
                path_or_url: arg("nested//path/"),
            }),
            PathBuf::from("/sync/root"),
            arg("alice"),
        )
        .expect("relative local path should normalize");
    assert!(relative.ends_with("/sync/root/nested/path"));

    let windows = command
        .normalize_peer_identity(
            PeerLocation::Local(LocalPeerTarget {
                path_or_url: arg("d:\\Data\\Tree\\"),
            }),
            PathBuf::from("/ignored"),
            arg("alice"),
        )
        .expect("windows drive path should normalize");
    assert!(
        windows == "file:///d:/Data/Tree" || windows == "file:///D:/Data/Tree",
        "windows drive path should normalize to a file URL without requiring drive-letter case"
    );

    let sftp = command
        .normalize_peer_identity(
            PeerLocation::Sftp(SftpPeerTarget {
                host: arg("HOST.Example"),
                username: arg("bob"),
                username_was_explicit: true,
                password: None,
                port: 2222,
                absolute_path: arg("//Root/%7Ekeep/%2Fslash/"),
            }),
            PathBuf::from("/ignored"),
            arg("alice"),
        )
        .expect("sftp target should normalize");
    assert_eq!(
        sftp,
        "sftp://bob@host.example:2222/Root/~keep/%2Fslash"
    );
}
