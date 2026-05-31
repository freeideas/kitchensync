use kitchensync::cli::{parse_invocation, CliInvocation, CliParseEnv};
use kitchensync::{PeerRole, RunRequest, Verbosity};

use std::env::temp_dir;

fn cli_env() -> CliParseEnv {
    CliParseEnv {
        current_dir: temp_dir(),
        current_user: "alice".to_string(),
    }
}

fn expect_run(result: CliInvocation) -> RunRequest {
    match result {
        CliInvocation::Run(request) => request,
        _ => panic!("expected a valid run invocation"),
    }
}

fn expect_invalid(result: CliInvocation) -> String {
    match result {
        CliInvocation::Invalid { error, .. } => error.message,
        _ => panic!("expected invalid invocation"),
    }
}

fn local_file_identity(env: &CliParseEnv, peer: &str) -> String {
    let absolute_peer = env
        .current_dir
        .join(peer)
        .to_string_lossy()
        .replace('\\', "/");
    format!("file://{absolute_peer}")
}

fn unix_file_identity(peer: &str) -> String {
    format!("file://{peer}")
}

// Not reasonably testable through `parse_invocation`:
// - exact process exit status and stdout/stderr routing belong to `run_process`.

#[test]
fn parse_invocation_empty_args_returns_help() {
    let env = cli_env();

    let result = parse_invocation(Vec::<&str>::new(), &env);

    assert!(matches!(
        result,
        CliInvocation::Help { help } if help == kitchensync::cli::help_text()
    ));
}

#[test]
fn parse_invocation_invalid_option_returns_help_text() {
    let env = cli_env();

    let result = parse_invocation(["--does-not-exist", "left", "right"], &env);
    match result {
        CliInvocation::Invalid { error, help } => {
            assert!(error.message.contains("unknown option"));
            assert_eq!(help, kitchensync::cli::help_text());
        }
        _ => panic!("expected invalid invocation"),
    }
}

#[test]
fn parse_invocation_rejects_too_few_peer_operands() {
    let env = cli_env();

    let message = expect_invalid(parse_invocation(["left"], &env));

    assert!(message.contains("too few peer operands"));
}

#[test]
fn parse_invocation_applies_default_configuration() {
    let env = cli_env();

    let request = expect_run(parse_invocation(["left", "right"], &env));

    assert_eq!(request.config.dry_run, false);
    assert_eq!(request.config.max_copies, 10);
    assert_eq!(request.config.retries_copy, 3);
    assert_eq!(request.config.retries_list, 3);
    assert_eq!(request.config.timeout_conn, 30);
    assert_eq!(request.config.timeout_idle, 30);
    assert_eq!(request.config.verbosity, Verbosity::Info);
    assert_eq!(request.config.keep_tmp_days, 2);
    assert_eq!(request.config.keep_bak_days, 90);
    assert_eq!(request.config.keep_del_days, 180);
    assert!(request.config.excludes.is_empty());

    assert_eq!(request.peers[0].role, PeerRole::Normal);
    assert_eq!(request.peers[1].role, PeerRole::Normal);
    assert_eq!(
        request.peers[0].urls[0].identity,
        local_file_identity(&env, "left")
    );
    assert_eq!(
        request.peers[1].urls[0].identity,
        local_file_identity(&env, "right")
    );
}

#[test]
fn parse_invocation_accepts_global_options_and_preserves_exclude_order() {
    let env = cli_env();

    let request = expect_run(parse_invocation(
        [
            "--dry-run",
            "--max-copies",
            "4",
            "--retries-copy",
            "2",
            "--retries-list",
            "1",
            "--timeout-conn",
            "45",
            "--timeout-idle",
            "60",
            "--verbosity",
            "trace",
            "--keep-tmp-days",
            "9",
            "--keep-bak-days",
            "10",
            "--keep-del-days",
            "11",
            "-x",
            "cache",
            "-x",
            "tmp",
            "left",
            "right",
        ],
        &env,
    ));

    assert!(request.config.dry_run);
    assert_eq!(request.config.max_copies, 4);
    assert_eq!(request.config.retries_copy, 2);
    assert_eq!(request.config.retries_list, 1);
    assert_eq!(request.config.timeout_conn, 45);
    assert_eq!(request.config.timeout_idle, 60);
    assert_eq!(request.config.verbosity, Verbosity::Trace);
    assert_eq!(request.config.keep_tmp_days, 9);
    assert_eq!(request.config.keep_bak_days, 10);
    assert_eq!(request.config.keep_del_days, 11);
    assert_eq!(request.config.excludes[0].as_str(), "cache");
    assert_eq!(request.config.excludes[1].as_str(), "tmp");
    assert_eq!(request.excludes[0].as_str(), "cache");
    assert_eq!(request.excludes[1].as_str(), "tmp");
}

#[test]
fn parse_invocation_rejects_missing_option_value() {
    let env = cli_env();

    let message = expect_invalid(parse_invocation(["--max-copies"], &env));

    assert!(message.contains("missing value for --max-copies"));
}

#[test]
fn parse_invocation_rejects_missing_value_for_all_value_options() {
    let env = cli_env();

    for option in [
        "--max-copies",
        "--retries-copy",
        "--retries-list",
        "--timeout-conn",
        "--timeout-idle",
        "--keep-tmp-days",
        "--keep-bak-days",
        "--keep-del-days",
    ] {
        let message = expect_invalid(parse_invocation([option], &env));
        assert!(message.contains(&format!("missing value for {option}")));
    }
}

#[test]
fn parse_invocation_rejects_zero_for_positive_integer_options() {
    let env = cli_env();

    let message = expect_invalid(parse_invocation(
        ["--retries-copy", "0", "left", "right"],
        &env,
    ));

    assert!(message.contains("requires a positive integer"));
}

#[test]
fn parse_invocation_rejects_zero_for_all_positive_integer_options() {
    let env = cli_env();

    for option in [
        "--max-copies",
        "--retries-copy",
        "--retries-list",
        "--timeout-conn",
        "--timeout-idle",
        "--keep-tmp-days",
        "--keep-bak-days",
        "--keep-del-days",
    ] {
        let message = expect_invalid(parse_invocation([option, "0", "left", "right"], &env));
        assert!(message.contains("requires a positive integer"));
    }
}

#[test]
fn parse_invocation_rejects_invalid_verbosity() {
    let env = cli_env();

    let message = expect_invalid(parse_invocation(
        ["--verbosity", "noisy", "left", "right"],
        &env,
    ));

    assert!(message.contains("unsupported verbosity"));
}

#[test]
fn parse_invocation_accepts_all_verbosity_values() {
    let env = cli_env();

    assert_eq!(
        expect_run(parse_invocation(
            ["--verbosity", "error", "left", "right"],
            &env
        ))
        .config
        .verbosity,
        Verbosity::Error
    );
    assert_eq!(
        expect_run(parse_invocation(
            ["--verbosity", "info", "left", "right"],
            &env
        ))
        .config
        .verbosity,
        Verbosity::Info
    );
    assert_eq!(
        expect_run(parse_invocation(
            ["--verbosity", "debug", "left", "right"],
            &env
        ))
        .config
        .verbosity,
        Verbosity::Debug
    );
    assert_eq!(
        expect_run(parse_invocation(
            ["--verbosity", "trace", "left", "right"],
            &env
        ))
        .config
        .verbosity,
        Verbosity::Trace
    );
}

#[test]
fn parse_invocation_rejects_negative_positive_integer_option_values() {
    let env = cli_env();

    for option in [
        "--max-copies",
        "--retries-copy",
        "--retries-list",
        "--timeout-conn",
        "--timeout-idle",
        "--keep-tmp-days",
        "--keep-bak-days",
        "--keep-del-days",
    ] {
        let message = expect_invalid(parse_invocation([option, "-1", "left", "right"], &env));
        assert!(message.contains("requires a positive integer"));
    }
}

#[test]
fn parse_invocation_rejects_unsupported_peer_url_form() {
    let env = cli_env();

    let message = expect_invalid(parse_invocation(
        ["http://left.example/path", "right", "third"],
        &env,
    ));

    assert!(message.contains("unsupported"));
}

#[test]
fn parse_invocation_accepts_excludes_after_peer_operands() {
    let env = cli_env();

    let request = expect_run(parse_invocation(
        ["left", "right", "-x", "cache", "-x", "tmp"],
        &env,
    ));

    assert_eq!(request.config.excludes[0].as_str(), "cache");
    assert_eq!(request.config.excludes[1].as_str(), "tmp");
}

#[test]
fn parse_invocation_rejects_invalid_excludes() {
    let env = cli_env();

    let message = expect_invalid(parse_invocation(["-x", "../bad", "left", "right"], &env));

    assert!(message.contains("invalid exclude path"));
}

#[test]
fn parse_invocation_rejects_excludes_with_disallowed_path_characters() {
    let env = cli_env();

    let leading_slash = expect_invalid(parse_invocation(["-x", "/bad", "left", "right"], &env));
    assert!(leading_slash.contains("invalid exclude path"));

    let backslash = expect_invalid(parse_invocation(["-x", "a\\b", "left", "right"], &env));
    assert!(backslash.contains("invalid exclude path"));
}

#[test]
fn parse_invocation_rejects_multiple_explicit_canon_peers() {
    let env = cli_env();

    let message = expect_invalid(parse_invocation(
        ["+left", "+right", "third", "fourth"],
        &env,
    ));

    assert!(message.contains("more than one canon peer"));
}

#[test]
fn parse_invocation_preserves_peer_and_fallback_order() {
    let env = cli_env();

    let request = expect_run(parse_invocation(
        ["+[left,right]", "-third", "fourth"],
        &env,
    ));

    assert_eq!(request.peers[0].role, PeerRole::Canon);
    assert_eq!(
        request.peers[0].urls[0].identity,
        local_file_identity(&env, "left")
    );
    assert_eq!(
        request.peers[0].urls[1].identity,
        local_file_identity(&env, "right")
    );
    assert_eq!(request.peers[1].role, PeerRole::Subordinate);
    assert_eq!(request.peers[2].role, PeerRole::Normal);
}

#[test]
fn parse_invocation_normalizes_file_url_identity() {
    let env = cli_env();

    let request = expect_run(parse_invocation(
        ["left//inner///", "FILE://localhost//C:/right///"],
        &env,
    ));

    let left = &request.peers[0].urls[0];
    let right = &request.peers[1].urls[0];

    assert_eq!(left.scheme, "file");
    assert!(left.host.is_none());
    assert_eq!(left.identity, local_file_identity(&env, "left/inner"));
    assert_eq!(right.scheme, "file");
    assert_eq!(right.path, "C:/right");
    assert_eq!(right.identity, "file://C:/right");
}

#[test]
fn parse_invocation_normalizes_unix_absolute_local_path() {
    let env = cli_env();

    let request = expect_run(parse_invocation(["/tmp/data/path", "left"], &env));

    assert_eq!(
        request.peers[0].urls[0].identity,
        unix_file_identity("/tmp/data/path")
    );
}

#[test]
fn parse_invocation_removes_trailing_slash_from_local_path() {
    let env = cli_env();

    let request = expect_run(parse_invocation(["/tmp/data/path/", "left"], &env));

    assert_eq!(
        request.peers[0].urls[0].identity,
        unix_file_identity("/tmp/data/path")
    );
}

#[test]
fn parse_invocation_preserves_non_default_sftp_port() {
    let env = cli_env();

    let request = expect_run(parse_invocation(
        ["sftp://user@host:2222/path?timeout-conn=9", "left", "right"],
        &env,
    ));

    let peer = &request.peers[0].urls[0];

    assert_eq!(peer.port, Some(2222));
    assert_eq!(peer.identity, "sftp://user@host:2222/path");
}

#[test]
fn parse_invocation_percent_decodes_unreserved_file_path_characters() {
    let env = cli_env();

    let request = expect_run(parse_invocation(["left%7Epath", "right"], &env));

    assert_eq!(
        request.peers[0].urls[0].identity,
        local_file_identity(&env, "left~path")
    );
}

#[test]
fn parse_invocation_normalizes_sftp_identity_and_url_settings() {
    let env = cli_env();

    let request = expect_run(parse_invocation(
        [
            "sftp://HOST/ROOT//?timeout-conn=12",
            "sftp://user@host:22/path/?timeout-idle=8",
            "sftp://alice:p%40ss@otherhost/ROOT?timeout-conn=2&timeout-idle=9",
        ],
        &env,
    ));

    let first = &request.peers[0].urls[0];
    let second = &request.peers[1].urls[0];
    let third = &request.peers[2].urls[0];

    assert_eq!(first.scheme, "sftp");
    assert_eq!(first.username.as_deref(), Some("alice"));
    assert_eq!(first.host.as_deref(), Some("host"));
    assert_eq!(first.path, "/ROOT");
    assert_eq!(first.identity, "sftp://alice@host/ROOT");
    assert_eq!(first.timeout_conn, Some(12));
    assert!(first.timeout_idle.is_none());
    assert_eq!(first.port, None);

    assert_eq!(second.username.as_deref(), Some("user"));
    assert_eq!(second.host.as_deref(), Some("host"));
    assert_eq!(second.port, None);
    assert_eq!(second.path, "/path");
    assert_eq!(second.identity, "sftp://user@host/path");
    assert_eq!(second.timeout_idle, Some(8));

    assert_eq!(third.username.as_deref(), Some("alice"));
    assert_eq!(third.password.as_deref(), Some("p%40ss"));
    assert_eq!(third.host.as_deref(), Some("otherhost"));
    assert_eq!(third.path, "/ROOT");
    assert_eq!(third.identity, "sftp://alice@otherhost/ROOT");
    assert_eq!(third.timeout_conn, Some(2));
    assert_eq!(third.timeout_idle, Some(9));
}

#[test]
fn parse_invocation_rejects_role_prefixes_inside_fallback_group() {
    let env = cli_env();

    let request = parse_invocation(["[+left,right]", "third", "fourth"], &env);
    assert!(matches!(request, CliInvocation::Invalid { .. }));
}

#[test]
fn parse_invocation_rejects_invalid_fallback_group_syntax() {
    let env = cli_env();

    let request = parse_invocation(["[left,right", "third", "fourth"], &env);
    assert!(matches!(request, CliInvocation::Invalid { .. }));
}

#[test]
fn parse_invocation_parses_fallback_group_with_windows_paths() {
    let env = cli_env();

    let request = expect_run(parse_invocation(
        ["left", "[right,C:/tmp/fallback]", "third"],
        &env,
    ));

    assert_eq!(
        request.peers[1].urls[0].identity,
        local_file_identity(&env, "right")
    );
    assert_eq!(request.peers[1].urls[1].identity, "file://C:/tmp/fallback");
}

#[test]
fn parse_invocation_rejects_unsupported_url_query_parameter_in_any_position() {
    let env = cli_env();

    let message = expect_invalid(parse_invocation(["left?max-copies=3", "right"], &env));
    assert!(message.contains("unsupported URL query parameter"));

    let non_numeric_message =
        expect_invalid(parse_invocation(["left?unsupported=value", "right"], &env));
    assert!(non_numeric_message.contains("unsupported URL query parameter"));
}

#[test]
fn parse_invocation_rejects_unsupported_or_missing_per_url_query_values() {
    let env = cli_env();

    let unsupported = expect_invalid(parse_invocation(["left?bad=10", "right"], &env));
    assert!(unsupported.contains("unsupported URL query parameter"));

    let missing_value = expect_invalid(parse_invocation(["left?timeout-idle=", "right"], &env));
    assert!(missing_value.contains("missing value for URL query parameter"));

    let non_integer = expect_invalid(parse_invocation(
        ["left?timeout-conn=not-a-number", "right"],
        &env,
    ));
    assert!(non_integer.contains("requires a positive integer"));

    let non_positive = expect_invalid(parse_invocation(["left?timeout-idle=0", "right"], &env));
    assert!(non_positive.contains("requires a positive integer"));
}

#[test]
fn parse_invocation_accepts_absolute_local_and_windows_paths_without_fallback() {
    let env = cli_env();

    let request = expect_run(parse_invocation(["left", "C:/tmp/fallback", "right"], &env));

    assert_eq!(request.peers[1].urls[0].identity, "file://C:/tmp/fallback");
}

#[test]
fn parse_invocation_preserves_per_url_query_scope_in_fallback_groups() {
    let env = cli_env();

    let request = expect_run(parse_invocation(
        [
            "left",
            "[right?timeout-idle=7,other?timeout-conn=8]",
            "[third?timeout-conn=5]",
        ],
        &env,
    ));

    assert_eq!(request.peers[1].urls.len(), 2);
    assert_eq!(request.peers[1].urls[0].timeout_idle, Some(7));
    assert!(request.peers[1].urls[0].timeout_conn.is_none());
    assert_eq!(request.peers[1].urls[1].timeout_conn, Some(8));
    assert!(request.peers[1].urls[1].timeout_idle.is_none());
    assert_eq!(request.peers[2].urls[0].timeout_conn, Some(5));
}
