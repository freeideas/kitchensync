use std::path::PathBuf;
use std::sync::Arc;

use commandandoutput::{
    CommandAndOutput, CommandParseResult, LocalPeerTarget, PeerLocation, PeerRole,
    SftpPeerTarget, UrlConnectionSettings, Verbosity,
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

fn help_text() -> String {
    let source = include_str!("../../../../specs/help.md");
    let start = source.find("```\n").expect("help fence starts") + 4;
    let end = source[start..].find("\n```").expect("help fence ends") + start;
    source[start..end].to_owned()
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
            arg("+[sftp://USER:p%40ss%3Aword@Host.Example:22//Root/%7EData/?timeout-conn=11,sftp://backup.example:2200/root?timeout-idle=12]"),
            arg("-C:\\sync\\subordinate"),
            arg("relative/normal"),
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

    assert_eq!(run.peers.len(), 3);
    assert_eq!(run.peers[0].role, PeerRole::Canon);
    assert_eq!(run.peers[1].role, PeerRole::Subordinate);
    assert_eq!(run.peers[2].role, PeerRole::Normal);

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
        "sftp://USER@host.example/Root/~Data"
    );
    let PeerLocation::Sftp(first_sftp) = &first_target.location else {
        panic!("expected first fallback to be sftp");
    };
    assert_eq!(first_sftp.host, "Host.Example");
    assert_eq!(first_sftp.username, "USER");
    assert_eq!(first_sftp.username_was_explicit, true);
    assert_eq!(first_sftp.password.as_deref(), Some("p@ss:word"));
    assert_eq!(first_sftp.port, 22);
    assert_eq!(first_sftp.absolute_path, "//Root/%7EData/");

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

    let PeerLocation::Local(windows_local) = &run.peers[1].fallback_targets[0].location else {
        panic!("expected windows drive path to be local");
    };
    assert_eq!(windows_local.path_or_url, "C:\\sync\\subordinate");
    assert!(
        run.peers[1].fallback_targets[0]
            .normalized_identity
            .starts_with("file:///C:/sync/subordinate")
    );

    let PeerLocation::Local(relative_local) = &run.peers[2].fallback_targets[0].location else {
        panic!("expected relative path to be local");
    };
    assert_eq!(relative_local.path_or_url, "relative/normal");
    assert!(
        run.peers[2].fallback_targets[0]
            .normalized_identity
            .ends_with("/current/root/relative/normal")
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
    let cases = [
        vec![arg("/only-one-peer")],
        vec![arg("+/one"), arg("+/two")],
        vec![arg("--unknown"), arg("/one"), arg("/two")],
        vec![arg("--max-copies"), arg("0"), arg("/one"), arg("/two")],
        vec![arg("--verbosity"), arg("verbose"), arg("/one"), arg("/two")],
        vec![arg("-x"), arg("../bad"), arg("/one"), arg("/two")],
        vec![arg("sftp://host/root?max-copies=2"), arg("/two")],
        vec![arg("sftp://host/root?timeout-conn=zero"), arg("/two")],
        vec![arg("--timeout-idle")],
    ];

    for args in cases {
        let result = subject().parse_command(args, PathBuf::from("/work"), arg("alice"));
        let CommandParseResult::ValidationFailure(output) = result else {
            panic!("expected validation failure");
        };

        assert!(output.stdout.ends_with(&help_text()));
        assert_ne!(output.stdout, help_text());
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 1);
    }
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
    assert_eq!(windows, "file:///D:/Data/Tree");

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
