use std::sync::Arc;

use commandandoutput_globalargumentparser::{
    GlobalArgumentParseResult, GlobalArgumentParser, GlobalRunSettings, GlobalVerbosity,
};

fn subject() -> Arc<dyn GlobalArgumentParser> {
    commandandoutput_globalargumentparser::new()
}

fn arg(value: &str) -> String {
    value.to_owned()
}

fn parse(args: Vec<String>) -> GlobalArgumentParseResult {
    subject().parse_global_arguments(args, help_text())
}

fn help_text() -> String {
    let source = include_str!("../../../../../../specs/help.md");
    let start = source.find("```\n").expect("help fence starts") + 4;
    let end = source[start..].find("\n```").expect("help fence ends") + start;
    source[start..end].to_owned()
}

fn run_request(args: Vec<String>) -> (GlobalRunSettings, Vec<String>) {
    let result = parse(args);
    let GlobalArgumentParseResult::Run(run) = result else {
        panic!("expected run request");
    };
    (run.settings, run.peer_operands)
}

fn assert_validation_failure(args: Vec<String>) {
    let result = parse(args);
    let GlobalArgumentParseResult::ValidationFailure(output) = result else {
        panic!("expected validation failure");
    };

    let expected_help = help_text();
    assert!(output.stdout.ends_with(&expected_help));
    assert_ne!(output.stdout, expected_help);
    assert_eq!(output.stderr, "");
    assert_eq!(output.exit_code, 1);
}

#[test]
fn no_arguments_return_verbatim_help_stdout_exit_zero_and_empty_stderr() {
    let result = parse(Vec::new());
    let GlobalArgumentParseResult::Help(output) = result else {
        panic!("expected help result");
    };

    assert_eq!(output.stdout, help_text());
    assert_eq!(output.stderr, "");
    assert_eq!(output.exit_code, 0);
}

#[test]
fn omitted_options_use_documented_defaults() {
    let (settings, peer_operands) = run_request(vec![arg("+/canon"), arg("/other")]);

    assert_eq!(settings.dry_run, false);
    assert_eq!(settings.max_copies, 10);
    assert_eq!(settings.retries_copy, 3);
    assert_eq!(settings.retries_list, 3);
    assert_eq!(settings.timeout_conn_seconds, 30);
    assert_eq!(settings.timeout_idle_seconds, 30);
    assert_eq!(settings.verbosity, GlobalVerbosity::Info);
    assert_eq!(settings.keep_tmp_days, 2);
    assert_eq!(settings.keep_bak_days, 90);
    assert_eq!(settings.keep_del_days, 180);
    assert_eq!(settings.excludes, Vec::<String>::new());
    assert_eq!(peer_operands, vec![arg("+/canon"), arg("/other")]);
}

#[test]
fn documented_options_before_peers_are_parsed_into_run_settings() {
    let (settings, peer_operands) = run_request(vec![
        arg("--dry-run"),
        arg("--max-copies"),
        arg("11"),
        arg("--retries-copy"),
        arg("4"),
        arg("--retries-list"),
        arg("5"),
        arg("--timeout-conn"),
        arg("41"),
        arg("--timeout-idle"),
        arg("51"),
        arg("--verbosity"),
        arg("trace"),
        arg("--keep-tmp-days"),
        arg("6"),
        arg("--keep-bak-days"),
        arg("92"),
        arg("--keep-del-days"),
        arg("181"),
        arg("-x"),
        arg("cache/tmp"),
        arg("-x"),
        arg("logs/archive"),
        arg("+/canon"),
        arg("/other"),
    ]);

    assert_eq!(settings.dry_run, true);
    assert_eq!(settings.max_copies, 11);
    assert_eq!(settings.retries_copy, 4);
    assert_eq!(settings.retries_list, 5);
    assert_eq!(settings.timeout_conn_seconds, 41);
    assert_eq!(settings.timeout_idle_seconds, 51);
    assert_eq!(settings.verbosity, GlobalVerbosity::Trace);
    assert_eq!(settings.keep_tmp_days, 6);
    assert_eq!(settings.keep_bak_days, 92);
    assert_eq!(settings.keep_del_days, 181);
    assert_eq!(settings.excludes, vec![arg("cache/tmp"), arg("logs/archive")]);
    assert_eq!(peer_operands, vec![arg("+/canon"), arg("/other")]);
}

#[test]
fn global_options_are_only_consumed_before_peer_operands() {
    let (settings, peer_operands) = run_request(vec![
        arg("--dry-run"),
        arg("+/canon"),
        arg("--max-copies"),
        arg("77"),
        arg("-x"),
        arg("after/peer"),
    ]);

    assert_eq!(settings.dry_run, true);
    assert_eq!(settings.max_copies, 10);
    assert_eq!(
        peer_operands,
        vec![
            arg("+/canon"),
            arg("--max-copies"),
            arg("77"),
            arg("-x"),
            arg("after/peer"),
        ]
    );
}

#[test]
fn positive_integer_options_accept_positive_values() {
    let cases = [
        ("--max-copies", 12),
        ("--retries-copy", 13),
        ("--retries-list", 14),
        ("--timeout-conn", 15),
        ("--timeout-idle", 16),
        ("--keep-tmp-days", 17),
        ("--keep-bak-days", 18),
        ("--keep-del-days", 19),
    ];

    for (option, expected) in cases {
        let (settings, _) = run_request(vec![arg(option), arg(&expected.to_string()), arg("/a")]);
        let actual = match option {
            "--max-copies" => settings.max_copies,
            "--retries-copy" => settings.retries_copy,
            "--retries-list" => settings.retries_list,
            "--timeout-conn" => settings.timeout_conn_seconds,
            "--timeout-idle" => settings.timeout_idle_seconds,
            "--keep-tmp-days" => settings.keep_tmp_days,
            "--keep-bak-days" => settings.keep_bak_days,
            "--keep-del-days" => settings.keep_del_days,
            _ => unreachable!(),
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn positive_integer_options_reject_invalid_values() {
    let options = [
        "--max-copies",
        "--retries-copy",
        "--retries-list",
        "--timeout-conn",
        "--timeout-idle",
        "--keep-tmp-days",
        "--keep-bak-days",
        "--keep-del-days",
    ];
    let invalid_values = ["0", "-1", "", "1.5", "many"];

    for option in options {
        for value in invalid_values {
            assert_validation_failure(vec![arg(option), arg(value), arg("/a")]);
        }
    }
}

#[test]
fn valued_global_options_fail_without_required_value() {
    for option in [
        "--max-copies",
        "--retries-copy",
        "--retries-list",
        "--timeout-conn",
        "--timeout-idle",
        "--verbosity",
        "-x",
        "--keep-tmp-days",
        "--keep-bak-days",
        "--keep-del-days",
    ] {
        assert_validation_failure(vec![arg(option)]);
    }
}

#[test]
fn verbosity_accepts_only_documented_levels() {
    let cases = [
        ("error", GlobalVerbosity::Error),
        ("info", GlobalVerbosity::Info),
        ("debug", GlobalVerbosity::Debug),
        ("trace", GlobalVerbosity::Trace),
    ];

    for (value, expected) in cases {
        let (settings, _) = run_request(vec![arg("--verbosity"), arg(value), arg("/a")]);
        assert_eq!(settings.verbosity, expected);
    }

    assert_validation_failure(vec![arg("--verbosity"), arg("warn"), arg("/a")]);
}

#[test]
fn repeated_excludes_accept_relative_slash_paths_in_order() {
    let (settings, _) = run_request(vec![
        arg("-x"),
        arg("cache/tmp"),
        arg("-x"),
        arg("one/two/three"),
        arg("/a"),
    ]);

    assert_eq!(settings.excludes, vec![arg("cache/tmp"), arg("one/two/three")]);
}

#[test]
fn excludes_reject_invalid_relative_slash_paths() {
    for value in [
        "/leading",
        "trailing/",
        "has\\backslash",
        "empty//segment",
        "dot/./segment",
        "dotdot/../segment",
        "has\0nul",
    ] {
        assert_validation_failure(vec![arg("-x"), arg(value), arg("/a")]);
    }
}

#[test]
fn unrecognized_flag_in_global_option_area_fails_validation() {
    assert_validation_failure(vec![arg("--unknown"), arg("/a")]);
}
