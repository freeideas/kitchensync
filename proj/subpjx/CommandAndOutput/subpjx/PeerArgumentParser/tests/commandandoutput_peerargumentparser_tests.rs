use commandandoutput_peerargumentparser::{
    new, PeerArgumentLocation, PeerArgumentParseResult, PeerArgumentParser,
    PeerArgumentPeerRole, PeerArgumentTarget, PeerArgumentUrlConnectionSettings,
    PeerArgumentValidationReason,
};
use std::sync::Arc;

fn subject() -> Arc<dyn PeerArgumentParser> {
    new()
}

fn parse(peer_operands: &[&str]) -> PeerArgumentParseResult {
    subject().parse_peer_arguments(
        peer_operands.iter().map(|value| value.to_string()).collect(),
        10,
        20,
        "currentuser".to_string(),
    )
}

fn parsed(peer_operands: &[&str]) -> Vec<commandandoutput_peerargumentparser::PeerArgumentPeer> {
    match parse(peer_operands) {
        PeerArgumentParseResult::Parsed(peers) => peers,
        other => panic!("expected successful parse, got {other:?}"),
    }
}

fn assert_validation_failure(
    peer_operands: &[&str],
    expected_reason: PeerArgumentValidationReason,
) {
    assert_eq!(
        parse(peer_operands),
        PeerArgumentParseResult::ValidationFailure(expected_reason)
    );
}

fn only_target(peer: &commandandoutput_peerargumentparser::PeerArgumentPeer) -> &PeerArgumentTarget {
    assert_eq!(peer.fallback_targets.len(), 1);
    &peer.fallback_targets[0]
}

fn assert_local_target(target: &PeerArgumentTarget, expected_path_or_url: &str) {
    assert_eq!(
        target.connection,
        PeerArgumentUrlConnectionSettings {
            timeout_conn_seconds: 10,
            timeout_idle_seconds: 20,
        }
    );
    match &target.location {
        PeerArgumentLocation::Local(local) => {
            assert_eq!(local.path_or_url, expected_path_or_url);
        }
        other => panic!("expected local target, got {other:?}"),
    }
}

#[test]
fn accepts_two_or_more_peer_operands_and_preserves_peer_order() {
    let peers = parsed(&["left", "right", "third"]);

    assert_eq!(peers.len(), 3);
    assert_local_target(only_target(&peers[0]), "left");
    assert_local_target(only_target(&peers[1]), "right");
    assert_local_target(only_target(&peers[2]), "third");
}

#[test]
fn rejects_non_help_invocations_with_fewer_than_two_peers() {
    assert_validation_failure(&[], PeerArgumentValidationReason::TooFewPeerOperands);
    assert_validation_failure(&["only"], PeerArgumentValidationReason::TooFewPeerOperands);
}

#[test]
fn parses_peer_roles_and_rejects_more_than_one_canon_peer() {
    let peers = parsed(&["+canon", "-subordinate-a", "-subordinate-b", "normal"]);

    assert_eq!(peers[0].role, PeerArgumentPeerRole::Canon);
    assert_eq!(peers[1].role, PeerArgumentPeerRole::Subordinate);
    assert_eq!(peers[2].role, PeerArgumentPeerRole::Subordinate);
    assert_eq!(peers[3].role, PeerArgumentPeerRole::Normal);
    assert_local_target(only_target(&peers[0]), "canon");
    assert_local_target(only_target(&peers[1]), "subordinate-a");
    assert_local_target(only_target(&peers[2]), "subordinate-b");
    assert_local_target(only_target(&peers[3]), "normal");

    assert_validation_failure(
        &["+first", "+second"],
        PeerArgumentValidationReason::MoreThanOneCanonPeer,
    );
}

#[test]
fn parses_bracketed_fallback_peers_in_member_order_with_one_outer_role() {
    let peers = parsed(&[
        "+[sftp://host/first,-literal,+literal]",
        "-[relative,sftp://user@remote/second]",
    ]);

    assert_eq!(peers[0].role, PeerArgumentPeerRole::Canon);
    assert_eq!(peers[0].fallback_targets.len(), 3);
    assert_eq!(peers[1].role, PeerArgumentPeerRole::Subordinate);
    assert_eq!(peers[1].fallback_targets.len(), 2);

    match &peers[0].fallback_targets[0].location {
        PeerArgumentLocation::Sftp(sftp) => {
            assert_eq!(sftp.host, "host");
            assert_eq!(sftp.username, "currentuser");
            assert_eq!(sftp.port, 22);
            assert_eq!(sftp.absolute_path, "/first");
        }
        other => panic!("expected sftp target, got {other:?}"),
    }
    assert_local_target(&peers[0].fallback_targets[1], "-literal");
    assert_local_target(&peers[0].fallback_targets[2], "+literal");
    assert_local_target(&peers[1].fallback_targets[0], "relative");
    match &peers[1].fallback_targets[1].location {
        PeerArgumentLocation::Sftp(sftp) => {
            assert_eq!(sftp.host, "remote");
            assert_eq!(sftp.username, "user");
            assert_eq!(sftp.port, 22);
            assert_eq!(sftp.absolute_path, "/second");
        }
        other => panic!("expected sftp target, got {other:?}"),
    }
}

#[test]
fn parses_local_paths_and_file_urls_as_local_targets() {
    let peers = parsed(&[
        "/var/data",
        "C:\\Users\\alice\\data",
        "relative/path",
        "file:///tmp/kitchensync",
    ]);

    assert_local_target(only_target(&peers[0]), "/var/data");
    assert_local_target(only_target(&peers[1]), "C:\\Users\\alice\\data");
    assert_local_target(only_target(&peers[2]), "relative/path");
    assert_local_target(only_target(&peers[3]), "file:///tmp/kitchensync");
}

#[test]
fn parses_sftp_url_fields_from_supported_forms() {
    let peers = parsed(&[
        "sftp://alice@example.test/docs",
        "sftp://bob@example.test:2200/rooted",
        "sftp://example.test/current-user",
        "sftp://carol:p%40ss%3Aword@example.test/secret",
    ]);

    match &only_target(&peers[0]).location {
        PeerArgumentLocation::Sftp(sftp) => {
            assert_eq!(sftp.host, "example.test");
            assert_eq!(sftp.username, "alice");
            assert_eq!(sftp.password, None);
            assert_eq!(sftp.port, 22);
            assert_eq!(sftp.absolute_path, "/docs");
        }
        other => panic!("expected sftp target, got {other:?}"),
    }
    match &only_target(&peers[1]).location {
        PeerArgumentLocation::Sftp(sftp) => {
            assert_eq!(sftp.username, "bob");
            assert_eq!(sftp.port, 2200);
            assert_eq!(sftp.absolute_path, "/rooted");
        }
        other => panic!("expected sftp target, got {other:?}"),
    }
    match &only_target(&peers[2]).location {
        PeerArgumentLocation::Sftp(sftp) => {
            assert_eq!(sftp.username, "currentuser");
            assert_eq!(sftp.port, 22);
            assert_eq!(sftp.absolute_path, "/current-user");
        }
        other => panic!("expected sftp target, got {other:?}"),
    }
    match &only_target(&peers[3]).location {
        PeerArgumentLocation::Sftp(sftp) => {
            assert_eq!(sftp.username, "carol");
            assert_eq!(sftp.password, Some("p@ss:word".to_string()));
            assert_eq!(sftp.absolute_path, "/secret");
        }
        other => panic!("expected sftp target, got {other:?}"),
    }
}

#[test]
fn parses_url_timeout_query_parameters_as_per_url_overrides() {
    let peers = parsed(&[
        "sftp://host/a?timeout-conn=30&timeout-idle=40",
        "sftp://host/b?timeout-conn=50",
        "sftp://host/c?timeout-idle=60",
        "file:///tmp/kitchensync?timeout-conn=70&timeout-idle=80",
    ]);

    assert_eq!(
        only_target(&peers[0]).connection,
        PeerArgumentUrlConnectionSettings {
            timeout_conn_seconds: 30,
            timeout_idle_seconds: 40,
        }
    );
    assert_eq!(
        only_target(&peers[1]).connection,
        PeerArgumentUrlConnectionSettings {
            timeout_conn_seconds: 50,
            timeout_idle_seconds: 20,
        }
    );
    assert_eq!(
        only_target(&peers[2]).connection,
        PeerArgumentUrlConnectionSettings {
            timeout_conn_seconds: 10,
            timeout_idle_seconds: 60,
        }
    );
    match &only_target(&peers[3]).location {
        PeerArgumentLocation::Local(local) => {
            assert_eq!(
                local.path_or_url,
                "file:///tmp/kitchensync?timeout-conn=70&timeout-idle=80"
            );
        }
        other => panic!("expected local target, got {other:?}"),
    }
    assert_eq!(
        only_target(&peers[3]).connection,
        PeerArgumentUrlConnectionSettings {
            timeout_conn_seconds: 70,
            timeout_idle_seconds: 80,
        }
    );
}

#[test]
fn rejects_unsupported_url_query_parameters() {
    assert_validation_failure(
        &["sftp://host/a?max-copies=2", "local"],
        PeerArgumentValidationReason::UnsupportedQueryParameter,
    );
    assert_validation_failure(
        &["sftp://host/a?other=2", "local"],
        PeerArgumentValidationReason::UnsupportedQueryParameter,
    );
    assert_validation_failure(
        &["file:///tmp/kitchensync?max-copies=2", "local"],
        PeerArgumentValidationReason::UnsupportedQueryParameter,
    );
}

#[test]
fn rejects_unsupported_peer_url_forms() {
    assert_validation_failure(
        &["https://host/path", "local"],
        PeerArgumentValidationReason::UnsupportedPeerUrlForm,
    );
}

#[test]
fn rejects_url_timeout_values_that_are_not_positive_integers() {
    for value in ["0", "-1", "", "1.5", "abc"] {
        let timeout_conn_operand = format!("sftp://host/a?timeout-conn={value}");
        assert_validation_failure(
            &[timeout_conn_operand.as_str(), "local"],
            PeerArgumentValidationReason::InvalidUrlTimeoutValue,
        );
        let timeout_idle_operand = format!("sftp://host/a?timeout-idle={value}");
        assert_validation_failure(
            &[timeout_idle_operand.as_str(), "local"],
            PeerArgumentValidationReason::InvalidUrlTimeoutValue,
        );
    }
}
