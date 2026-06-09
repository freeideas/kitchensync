use cli::{Cli, CliOutcome, PeerRole, Verbosity, new};

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn parse(a: &[&str]) -> CliOutcome {
    new().parse(args(a))
}

fn is_reject(outcome: CliOutcome) -> bool {
    matches!(outcome, CliOutcome::Reject(_))
}

fn is_run(outcome: CliOutcome) -> bool {
    matches!(outcome, CliOutcome::Run(_))
}

// Extract the content of the first fenced code block from a Markdown string.
fn extract_code_block(md: &str) -> &str {
    let start = md.find("```\n").expect("no opening code fence") + 4;
    let rest = &md[start..];
    let end = rest.find("\n```").expect("no closing code fence");
    &rest[..end]
}

// 001.1, 001.2: No arguments produces Help (caller prints help, exits 0).
#[test]
fn no_args_returns_help() {
    match new().parse(vec![]) {
        CliOutcome::Help => {}
        CliOutcome::Run(_) => panic!("expected Help, got Run"),
        CliOutcome::Reject(msg) => panic!("expected Help, got Reject: {}", msg),
    }
}

// 002.1: help_text() returns the verbatim content of specs/help.md, character for character.
#[test]
fn help_text_matches_spec() {
    let md = include_str!("../../../../specs/help.md");
    let expected = extract_code_block(md);
    let cli = new();
    assert_eq!(cli.help_text(), expected);
}

// 002.2, 002.3: No-argument case is Help (exits 0, stderr empty -- Cli never touches stderr).
#[test]
fn no_args_is_help_outcome() {
    assert!(matches!(new().parse(vec![]), CliOutcome::Help));
}

// 001.3, 001.5: A validation failure returns Reject carrying a non-empty error message.
#[test]
fn validation_error_returns_reject_with_message() {
    match parse(&["/only/one/peer"]) {
        CliOutcome::Reject(msg) => assert!(!msg.is_empty(), "error message must not be empty"),
        _ => panic!("expected Reject for one-peer invocation"),
    }
}

// 001.4, 002.4: The Reject message is the error only; help_text() provides the help text
// the caller must print after the error.
#[test]
fn reject_message_does_not_embed_help_text() {
    let cli = new();
    let help = cli.help_text();
    match cli.parse(args(&["/only/one/peer"])) {
        CliOutcome::Reject(msg) => {
            assert!(
                !msg.contains(&help),
                "Reject message must not embed the help text; caller prints it separately"
            );
        }
        _ => panic!("expected Reject"),
    }
}

// 001.6: Bare Unix path (no scheme) accepted as a local peer.
#[test]
fn bare_unix_path_accepted() {
    assert!(is_run(parse(&["/usr/local/data", "/mnt/backup"])));
}

// 001.6: Bare relative path accepted as a local peer.
#[test]
fn bare_relative_path_accepted() {
    assert!(is_run(parse(&["./local/data", "/mnt/backup"])));
}

// 001.6: Bare Windows-style path accepted as a local peer.
#[test]
fn bare_windows_path_accepted() {
    assert!(is_run(parse(&[r"c:\photos", "/mnt/backup"])));
}

// 001.7: sftp:// URL accepted as a peer.
#[test]
fn sftp_url_accepted() {
    assert!(is_run(parse(&["sftp://user@host/path", "/local"])));
}

// 001.8: Fewer than two peers is a validation error.
#[test]
fn one_peer_rejected() {
    assert!(is_reject(parse(&["/only/one"])));
}

// 001.8: Zero peers (flags only, no peers) is a validation error.
#[test]
fn zero_peers_rejected() {
    assert!(is_reject(parse(&["--dry-run"])));
}

// 001.9: + prefix marks the peer as Canon.
#[test]
fn plus_prefix_is_canon_role() {
    match parse(&["+/canon", "/normal"]) {
        CliOutcome::Run(cfg) => {
            let canon = cfg.peers.iter().find(|p| matches!(p.role, PeerRole::Canon));
            assert!(canon.is_some(), "expected a Canon peer");
        }
        other => panic!("expected Run, got {:?}", match other {
            CliOutcome::Reject(m) => format!("Reject({})", m),
            _ => "Help".to_string(),
        }),
    }
}

// 001.10: - prefix marks the peer as Subordinate.
#[test]
fn minus_prefix_is_subordinate_role() {
    match parse(&["/normal", "-/sub"]) {
        CliOutcome::Run(cfg) => {
            let sub = cfg.peers.iter().find(|p| matches!(p.role, PeerRole::Subordinate));
            assert!(sub.is_some(), "expected a Subordinate peer");
        }
        _ => panic!("expected Run"),
    }
}

// 001.11: No prefix marks the peer as Normal.
#[test]
fn no_prefix_is_normal_role() {
    match parse(&["/a", "/b"]) {
        CliOutcome::Run(cfg) => {
            for peer in &cfg.peers {
                assert!(
                    matches!(peer.role, PeerRole::Normal),
                    "expected Normal role for unprefixed peer"
                );
            }
        }
        _ => panic!("expected Run"),
    }
}

// 001.12: More than one + peer is a validation error.
#[test]
fn two_canon_peers_rejected() {
    assert!(is_reject(parse(&["+/a", "+/b"])));
}

// 001.13: Multiple - peers in one invocation are accepted.
#[test]
fn multiple_subordinate_peers_accepted() {
    match parse(&["/primary", "-/sub1", "-/sub2"]) {
        CliOutcome::Run(cfg) => {
            let count = cfg
                .peers
                .iter()
                .filter(|p| matches!(p.role, PeerRole::Subordinate))
                .count();
            assert_eq!(count, 2, "expected two Subordinate peers");
        }
        _ => panic!("expected Run"),
    }
}

// 001.14: Square brackets group comma-separated URLs into a single peer.
#[test]
fn bracketed_group_is_single_peer_with_multiple_urls() {
    match parse(&["[sftp://host-a/p,sftp://host-b/p]", "/local"]) {
        CliOutcome::Run(cfg) => {
            let group = cfg.peers.iter().find(|p| p.urls.len() > 1);
            assert!(group.is_some(), "expected a peer with multiple URLs");
            assert_eq!(group.unwrap().urls.len(), 2);
        }
        _ => panic!("expected Run"),
    }
}

// 001.15: + before a bracketed group makes the whole group Canon.
#[test]
fn plus_before_bracket_is_canon_group() {
    match parse(&["+[sftp://a/p,sftp://b/p]", "/local"]) {
        CliOutcome::Run(cfg) => {
            let canon = cfg.peers.iter().find(|p| matches!(p.role, PeerRole::Canon));
            assert!(canon.is_some(), "expected Canon peer");
            assert!(canon.unwrap().urls.len() > 1, "expected multiple URLs in Canon group");
        }
        _ => panic!("expected Run"),
    }
}

// 001.15: - before a bracketed group makes the whole group Subordinate.
#[test]
fn minus_before_bracket_is_subordinate_group() {
    match parse(&["/local", "-[sftp://a/p,sftp://b/p]"]) {
        CliOutcome::Run(cfg) => {
            let sub = cfg.peers.iter().find(|p| matches!(p.role, PeerRole::Subordinate));
            assert!(sub.is_some(), "expected Subordinate peer");
            assert!(sub.unwrap().urls.len() > 1, "expected multiple URLs in Subordinate group");
        }
        _ => panic!("expected Run"),
    }
}

// 001.16: timeout-conn query parameter on a peer URL is accepted.
#[test]
fn timeout_conn_query_param_accepted() {
    match parse(&["sftp://host/path?timeout-conn=60", "/local"]) {
        CliOutcome::Run(cfg) => {
            let settings = &cfg.peers[0].urls[0].settings;
            assert_eq!(settings.timeout_conn, Some(60));
        }
        _ => panic!("expected Run"),
    }
}

// 001.17: timeout-idle query parameter on a peer URL is accepted.
#[test]
fn timeout_idle_query_param_accepted() {
    match parse(&["sftp://host/path?timeout-idle=10", "/local"]) {
        CliOutcome::Run(cfg) => {
            let settings = &cfg.peers[0].urls[0].settings;
            assert_eq!(settings.timeout_idle, Some(10));
        }
        _ => panic!("expected Run"),
    }
}

// 001.18: A query parameter other than timeout-conn or timeout-idle is a validation error.
#[test]
fn unknown_query_param_rejected() {
    assert!(is_reject(parse(&["sftp://host/path?foo=bar", "/local"])));
}

// 001.19: max-copies as a query parameter on a peer URL is a validation error.
#[test]
fn max_copies_as_query_param_rejected() {
    assert!(is_reject(parse(&["sftp://host/path?max-copies=5", "/local"])));
}

// 001.20: All documented option flags are recognized and accepted.
#[test]
fn all_recognized_flags_accepted() {
    let outcome = parse(&[
        "--dry-run",
        "--max-copies", "5",
        "--retries-copy", "2",
        "--retries-list", "2",
        "--timeout-conn", "30",
        "--timeout-idle", "30",
        "--verbosity", "info",
        "-x", "tmp/cache",
        "--keep-tmp-days", "2",
        "--keep-bak-days", "90",
        "--keep-del-days", "180",
        "/path/a",
        "/path/b",
    ]);
    match outcome {
        CliOutcome::Run(_) => {}
        CliOutcome::Reject(msg) => panic!("recognized flags triggered Reject: {}", msg),
        CliOutcome::Help => panic!("expected Run, got Help"),
    }
}

// 001.21: An unrecognized flag is a validation error.
#[test]
fn unrecognized_flag_rejected() {
    assert!(is_reject(parse(&["--no-such-flag", "/path/a", "/path/b"])));
}

// 001.22: Zero value for --max-copies is a validation error.
#[test]
fn zero_max_copies_rejected() {
    assert!(is_reject(parse(&["--max-copies", "0", "/path/a", "/path/b"])));
}

// 001.22: Zero value for --retries-copy is a validation error.
#[test]
fn zero_retries_copy_rejected() {
    assert!(is_reject(parse(&["--retries-copy", "0", "/path/a", "/path/b"])));
}

// 001.22: Zero value for --retries-list is a validation error.
#[test]
fn zero_retries_list_rejected() {
    assert!(is_reject(parse(&["--retries-list", "0", "/path/a", "/path/b"])));
}

// 001.22: Zero value for --timeout-conn is a validation error.
#[test]
fn zero_timeout_conn_rejected() {
    assert!(is_reject(parse(&["--timeout-conn", "0", "/path/a", "/path/b"])));
}

// 001.22: Zero value for --timeout-idle is a validation error.
#[test]
fn zero_timeout_idle_rejected() {
    assert!(is_reject(parse(&["--timeout-idle", "0", "/path/a", "/path/b"])));
}

// 001.22: Zero value for --keep-tmp-days is a validation error.
#[test]
fn zero_keep_tmp_days_rejected() {
    assert!(is_reject(parse(&["--keep-tmp-days", "0", "/path/a", "/path/b"])));
}

// 001.22: Zero value for --keep-bak-days is a validation error.
#[test]
fn zero_keep_bak_days_rejected() {
    assert!(is_reject(parse(&["--keep-bak-days", "0", "/path/a", "/path/b"])));
}

// 001.22: Zero value for --keep-del-days is a validation error.
#[test]
fn zero_keep_del_days_rejected() {
    assert!(is_reject(parse(&["--keep-del-days", "0", "/path/a", "/path/b"])));
}

// 001.23: Non-integer value for --max-copies is a validation error.
#[test]
fn non_integer_max_copies_rejected() {
    assert!(is_reject(parse(&["--max-copies", "abc", "/path/a", "/path/b"])));
}

// 001.23: Float value for --retries-copy is a validation error.
#[test]
fn float_retries_copy_rejected() {
    assert!(is_reject(parse(&["--retries-copy", "1.5", "/path/a", "/path/b"])));
}

// 001.23: Non-integer value for --timeout-conn is a validation error.
#[test]
fn non_integer_timeout_conn_rejected() {
    assert!(is_reject(parse(&["--timeout-conn", "ten", "/path/a", "/path/b"])));
}

// 001.24: --verbosity accepts "error".
#[test]
fn verbosity_error_accepted() {
    match parse(&["--verbosity", "error", "/path/a", "/path/b"]) {
        CliOutcome::Run(cfg) => assert!(matches!(cfg.options.verbosity, Verbosity::Error)),
        _ => panic!("expected Run for --verbosity error"),
    }
}

// 001.24: --verbosity accepts "info".
#[test]
fn verbosity_info_accepted() {
    match parse(&["--verbosity", "info", "/path/a", "/path/b"]) {
        CliOutcome::Run(cfg) => assert!(matches!(cfg.options.verbosity, Verbosity::Info)),
        _ => panic!("expected Run for --verbosity info"),
    }
}

// 001.24: --verbosity accepts "debug".
#[test]
fn verbosity_debug_accepted() {
    match parse(&["--verbosity", "debug", "/path/a", "/path/b"]) {
        CliOutcome::Run(cfg) => assert!(matches!(cfg.options.verbosity, Verbosity::Debug)),
        _ => panic!("expected Run for --verbosity debug"),
    }
}

// 001.24: --verbosity accepts "trace".
#[test]
fn verbosity_trace_accepted() {
    match parse(&["--verbosity", "trace", "/path/a", "/path/b"]) {
        CliOutcome::Run(cfg) => assert!(matches!(cfg.options.verbosity, Verbosity::Trace)),
        _ => panic!("expected Run for --verbosity trace"),
    }
}

// 001.25: A --verbosity value other than the four accepted words is a validation error.
#[test]
fn invalid_verbosity_rejected() {
    assert!(is_reject(parse(&["--verbosity", "verbose", "/path/a", "/path/b"])));
}

// 001.26: -x <relative-path> is accepted and recorded in the exclude list.
#[test]
fn exclude_path_accepted() {
    match parse(&["-x", "some/relative/path", "/path/a", "/path/b"]) {
        CliOutcome::Run(cfg) => {
            assert!(
                cfg.excludes.contains(&"some/relative/path".to_string()),
                "exclude path not found in config"
            );
        }
        _ => panic!("expected Run"),
    }
}

// 001.27: Multiple -x flags are accepted; all paths are collected.
#[test]
fn multiple_excludes_collected() {
    match parse(&["-x", "dir1", "-x", "dir2/sub", "/path/a", "/path/b"]) {
        CliOutcome::Run(cfg) => {
            assert_eq!(cfg.excludes.len(), 2, "expected two excludes");
            assert!(cfg.excludes.contains(&"dir1".to_string()));
            assert!(cfg.excludes.contains(&"dir2/sub".to_string()));
        }
        _ => panic!("expected Run"),
    }
}

// 001.28: -x path with a leading / is a validation error.
#[test]
fn exclude_leading_slash_rejected() {
    assert!(is_reject(parse(&["-x", "/absolute/path", "/path/a", "/path/b"])));
}

// 001.29: -x path with a trailing / is a validation error.
#[test]
fn exclude_trailing_slash_rejected() {
    assert!(is_reject(parse(&["-x", "dir/subdir/", "/path/a", "/path/b"])));
}

// 001.30: -x path containing a \ separator is a validation error.
#[test]
fn exclude_backslash_rejected() {
    assert!(is_reject(parse(&["-x", r"dir\subdir", "/path/a", "/path/b"])));
}

// 001.31: -x path containing a . segment is a validation error.
#[test]
fn exclude_dot_segment_rejected() {
    assert!(is_reject(parse(&["-x", "dir/./subdir", "/path/a", "/path/b"])));
}

// 001.31: -x path containing a .. segment is a validation error.
#[test]
fn exclude_dotdot_segment_rejected() {
    assert!(is_reject(parse(&["-x", "dir/../other", "/path/a", "/path/b"])));
}

// 001.31: -x path containing an empty segment (double slash) is a validation error.
#[test]
fn exclude_empty_segment_rejected() {
    assert!(is_reject(parse(&["-x", "dir//subdir", "/path/a", "/path/b"])));
}

// 001.32: -x path containing a NUL character is a validation error.
#[test]
fn exclude_nul_character_rejected() {
    assert!(is_reject(parse(&["-x", "dir\x00file", "/path/a", "/path/b"])));
}
