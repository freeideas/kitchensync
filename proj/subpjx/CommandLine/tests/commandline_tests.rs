use commandline::{
    new, CommandLine, CommandLineParseResult, CommandLinePeerRole,
    CommandLineUrlAlternative, CommandLineVerbosity,
};

fn text(value: &str) -> String {
    value.to_string()
}

fn parse(args: &[&str]) -> CommandLineParseResult {
    new().parse(args.iter().map(|arg| text(arg)).collect())
}

fn run(args: &[&str]) -> commandline::CommandLineRunRequest {
    match parse(args) {
        CommandLineParseResult::Run(request) => request,
        other => panic!("expected run request, got {other:?}"),
    }
}

fn validation_error(args: &[&str]) -> commandline::CommandLineValidationError {
    match parse(args) {
        CommandLineParseResult::ValidationError(error) => error,
        other => panic!("expected validation error, got {other:?}"),
    }
}

fn help_screen() -> &'static str {
    let help_spec = include_str!("../../../../specs/help.md");
    help_spec
        .split_once("```\n")
        .and_then(|(_, rest)| rest.split_once("```"))
        .map(|(screen, _)| screen)
        .expect("help.md must contain the verbatim help screen")
}

#[test]
fn no_arguments_parse_as_help() {
    assert_eq!(parse(&[]), CommandLineParseResult::Help);
}

#[test]
fn help_output_is_the_verbatim_help_screen_with_success_exit() {
    let output = new().help_output();

    assert_eq!(output.stdout, help_screen());
    assert_eq!(output.exit_code, 0);
}

#[test]
fn validation_error_output_prints_one_error_then_help_and_exits_one() {
    let subject = new();
    let error = validation_error(&["only-one-peer"]);
    let output = subject.validation_error_output(&error);

    assert!(output.stdout.starts_with(&error.message));
    assert!(output.stdout.contains(help_screen()));
    assert_eq!(output.stdout.matches(help_screen()).count(), 1);
    assert_eq!(output.exit_code, 1);
    assert_eq!(output, subject.validation_error_output(&error));
}

#[test]
fn sync_complete_output_is_one_completion_line_with_success_exit() {
    let subject = new();
    let output = subject.sync_complete_output();

    assert_eq!(output.stdout, "sync complete\n");
    assert_eq!(output.exit_code, 0);
    assert_eq!(output, subject.sync_complete_output());
}

#[test]
fn omitted_global_options_use_documented_defaults() {
    let request = run(&["left", "right"]);

    assert!(!request.settings.dry_run);
    assert_eq!(request.settings.max_copies, 10);
    assert_eq!(request.settings.retries_copy, 3);
    assert_eq!(request.settings.retries_list, 3);
    assert_eq!(request.settings.timeout_conn_seconds, 30);
    assert_eq!(request.settings.timeout_idle_seconds, 30);
    assert_eq!(request.settings.verbosity, CommandLineVerbosity::Info);
    assert_eq!(request.settings.excludes, Vec::<String>::new());
    assert_eq!(request.settings.keep_tmp_days, 2);
    assert_eq!(request.settings.keep_bak_days, 90);
    assert_eq!(request.settings.keep_del_days, 180);
}

#[test]
fn global_options_accept_documented_values() {
    let request = run(&[
        "--dry-run",
        "--max-copies",
        "12",
        "--retries-copy",
        "4",
        "--retries-list",
        "5",
        "--timeout-conn",
        "60",
        "--timeout-idle",
        "70",
        "--verbosity",
        "trace",
        "-x",
        "cache/tmp",
        "-x",
        "logs",
        "--keep-tmp-days",
        "6",
        "--keep-bak-days",
        "91",
        "--keep-del-days",
        "181",
        "left",
        "right",
    ]);

    assert!(request.settings.dry_run);
    assert_eq!(request.settings.max_copies, 12);
    assert_eq!(request.settings.retries_copy, 4);
    assert_eq!(request.settings.retries_list, 5);
    assert_eq!(request.settings.timeout_conn_seconds, 60);
    assert_eq!(request.settings.timeout_idle_seconds, 70);
    assert_eq!(request.settings.verbosity, CommandLineVerbosity::Trace);
    assert_eq!(
        request.settings.excludes,
        vec![text("cache/tmp"), text("logs")]
    );
    assert_eq!(request.settings.keep_tmp_days, 6);
    assert_eq!(request.settings.keep_bak_days, 91);
    assert_eq!(request.settings.keep_del_days, 181);
}

#[test]
fn every_documented_verbosity_value_is_accepted() {
    let cases = [
        ("error", CommandLineVerbosity::Error),
        ("info", CommandLineVerbosity::Info),
        ("debug", CommandLineVerbosity::Debug),
        ("trace", CommandLineVerbosity::Trace),
    ];

    for (arg, expected) in cases {
        let request = run(&["--verbosity", arg, "left", "right"]);

        assert_eq!(request.settings.verbosity, expected);
    }
}

#[test]
fn positive_integer_options_reject_zero_negative_and_non_integer_values() {
    let option_names = [
        "--max-copies",
        "--retries-copy",
        "--retries-list",
        "--timeout-conn",
        "--timeout-idle",
        "--keep-tmp-days",
        "--keep-bak-days",
        "--keep-del-days",
    ];

    for option_name in option_names {
        for invalid_value in ["0", "-1", "abc"] {
            assert!(
                matches!(
                    parse(&[option_name, invalid_value, "left", "right"]),
                    CommandLineParseResult::ValidationError(_)
                ),
                "{option_name} accepted invalid value {invalid_value}"
            );
        }
    }
}

#[test]
fn unrecognized_flags_are_validation_errors() {
    assert!(matches!(
        parse(&["--unknown", "left", "right"]),
        CommandLineParseResult::ValidationError(_)
    ));
}

#[test]
fn excludes_must_be_relative_slash_paths() {
    for invalid_exclude in [
        "/abs",
        "trailing/",
        "a\\b",
        "a//b",
        ".",
        "a/.",
        "..",
        "a/..",
    ] {
        assert!(
            matches!(
                parse(&["-x", invalid_exclude, "left", "right"]),
                CommandLineParseResult::ValidationError(_)
            ),
            "accepted invalid exclude {invalid_exclude}"
        );
    }
}

#[test]
fn complete_run_requires_at_least_two_peers() {
    assert!(matches!(
        parse(&["only-one-peer"]),
        CommandLineParseResult::ValidationError(_)
    ));
}

#[test]
fn complete_run_accepts_peer_roles_and_keeps_peer_order() {
    let request = run(&["+canon", "normal", "-sub-a", "-sub-b"]);

    assert_eq!(request.peers.len(), 4);
    assert_eq!(request.peers[0].role, CommandLinePeerRole::Canon);
    assert_eq!(request.peers[1].role, CommandLinePeerRole::Normal);
    assert_eq!(request.peers[2].role, CommandLinePeerRole::Subordinate);
    assert_eq!(request.peers[3].role, CommandLinePeerRole::Subordinate);
}

#[test]
fn complete_run_rejects_more_than_one_canon_peer() {
    assert!(matches!(
        parse(&["+canon-a", "+canon-b"]),
        CommandLineParseResult::ValidationError(_)
    ));
}

#[test]
fn fallback_peer_arguments_are_one_peer_with_ordered_url_alternatives() {
    let request = run(&[
        "+[sftp://host/a,sftp://host/b]",
        "-[sftp://host/c,sftp://host/d]",
        "[sftp://host/e,sftp://host/f]",
    ]);

    assert_eq!(request.peers.len(), 3);
    assert_eq!(request.peers[0].role, CommandLinePeerRole::Canon);
    assert_eq!(
        request.peers[0].urls,
        vec![
            CommandLineUrlAlternative {
                url: text("sftp://host/a"),
                timeout_conn_seconds: None,
                timeout_idle_seconds: None,
            },
            CommandLineUrlAlternative {
                url: text("sftp://host/b"),
                timeout_conn_seconds: None,
                timeout_idle_seconds: None,
            },
        ]
    );
    assert_eq!(request.peers[1].role, CommandLinePeerRole::Subordinate);
    assert_eq!(
        request.peers[1]
            .urls
            .iter()
            .map(|alternative| alternative.url.as_str())
            .collect::<Vec<_>>(),
        vec!["sftp://host/c", "sftp://host/d"]
    );
    assert_eq!(request.peers[2].role, CommandLinePeerRole::Normal);
    assert_eq!(
        request.peers[2]
            .urls
            .iter()
            .map(|alternative| alternative.url.as_str())
            .collect::<Vec<_>>(),
        vec!["sftp://host/e", "sftp://host/f"]
    );
}

#[test]
fn bare_local_paths_are_accepted_as_file_urls() {
    let request = run(&["/absolute", "c:\\absolute", "./relative"]);

    assert_eq!(request.peers.len(), 3);
    for peer in request.peers {
        assert_eq!(peer.urls.len(), 1);
        assert!(
            peer.urls[0].url.starts_with("file://"),
            "local path was not represented as a file URL: {:?}",
            peer.urls[0].url
        );
    }
}

#[test]
fn documented_sftp_url_forms_are_accepted() {
    let request = run(&[
        "sftp://user@host/path",
        "sftp://user@host:2222/path",
        "sftp://host/path",
        "sftp://user:password@host/path",
        "sftp://user:p%40ss%3Aword@host/path",
    ]);

    assert_eq!(
        request
            .peers
            .iter()
            .map(|peer| peer.urls[0].url.as_str())
            .collect::<Vec<_>>(),
        vec![
            "sftp://user@host/path",
            "sftp://user@host:2222/path",
            "sftp://host/path",
            "sftp://user:password@host/path",
            "sftp://user:p%40ss%3Aword@host/path",
        ]
    );
}

#[test]
fn per_url_timeout_settings_are_accepted_and_carried() {
    let request = run(&[
        "sftp://host/a?timeout-conn=45&timeout-idle=55",
        "sftp://host/b",
    ]);

    assert_eq!(request.peers[0].urls.len(), 1);
    assert_eq!(request.peers[0].urls[0].timeout_conn_seconds, Some(45));
    assert_eq!(request.peers[0].urls[0].timeout_idle_seconds, Some(55));
}

#[test]
fn unknown_url_query_parameters_are_validation_errors() {
    assert!(matches!(
        parse(&["sftp://host/a?unknown=1", "sftp://host/b"]),
        CommandLineParseResult::ValidationError(_)
    ));
}

#[test]
fn parse_result_is_exactly_one_variant() {
    assert!(matches!(parse(&[]), CommandLineParseResult::Help));
    assert!(matches!(
        parse(&["only-one-peer"]),
        CommandLineParseResult::ValidationError(_)
    ));
    assert!(matches!(parse(&["left", "right"]), CommandLineParseResult::Run(_)));
}

#[test]
fn should_emit_error_at_every_configured_verbosity() {
    let subject = new();

    for configured in [
        CommandLineVerbosity::Error,
        CommandLineVerbosity::Info,
        CommandLineVerbosity::Debug,
        CommandLineVerbosity::Trace,
    ] {
        assert!(subject.should_emit(configured, CommandLineVerbosity::Error));
    }
}

#[test]
fn should_emit_each_lower_verbosity_at_higher_verbosity() {
    let subject = new();

    assert!(subject.should_emit(
        CommandLineVerbosity::Info,
        CommandLineVerbosity::Error
    ));
    assert!(subject.should_emit(
        CommandLineVerbosity::Info,
        CommandLineVerbosity::Info
    ));
    assert!(subject.should_emit(
        CommandLineVerbosity::Debug,
        CommandLineVerbosity::Error
    ));
    assert!(subject.should_emit(
        CommandLineVerbosity::Debug,
        CommandLineVerbosity::Info
    ));
    assert!(subject.should_emit(
        CommandLineVerbosity::Trace,
        CommandLineVerbosity::Error
    ));
    assert!(subject.should_emit(
        CommandLineVerbosity::Trace,
        CommandLineVerbosity::Info
    ));
    assert!(subject.should_emit(
        CommandLineVerbosity::Trace,
        CommandLineVerbosity::Debug
    ));
    assert!(subject.should_emit(
        CommandLineVerbosity::Trace,
        CommandLineVerbosity::Trace
    ));
}

#[test]
fn should_not_emit_messages_above_the_configured_verbosity() {
    let subject = new();

    assert!(!subject.should_emit(
        CommandLineVerbosity::Error,
        CommandLineVerbosity::Info
    ));
    assert!(!subject.should_emit(
        CommandLineVerbosity::Error,
        CommandLineVerbosity::Debug
    ));
    assert!(!subject.should_emit(
        CommandLineVerbosity::Error,
        CommandLineVerbosity::Trace
    ));
    assert!(!subject.should_emit(
        CommandLineVerbosity::Info,
        CommandLineVerbosity::Debug
    ));
    assert!(!subject.should_emit(
        CommandLineVerbosity::Info,
        CommandLineVerbosity::Trace
    ));
}

#[test]
fn debug_verbosity_currently_matches_info_observable_output() {
    let subject = new();
    let message_levels = [
        CommandLineVerbosity::Error,
        CommandLineVerbosity::Info,
        CommandLineVerbosity::Debug,
        CommandLineVerbosity::Trace,
    ];

    for message_level in message_levels {
        assert_eq!(
            subject.should_emit(CommandLineVerbosity::Debug, message_level),
            subject.should_emit(CommandLineVerbosity::Info, message_level)
        );
    }
}
