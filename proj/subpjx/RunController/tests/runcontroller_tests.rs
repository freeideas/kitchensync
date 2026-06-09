use std::path::{Path, PathBuf};
use std::sync::Arc;

use cli::{GlobalOptions, Peer, PeerRole, PeerUrl, RunConfig, UrlSettings, Verbosity};
use runcontroller::RunController;

fn fresh_dir(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("ks_rc_{}", tag));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn local_url(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

fn make_peer(role: PeerRole, url: String) -> Peer {
    Peer {
        role,
        urls: vec![PeerUrl {
            url,
            settings: UrlSettings {
                timeout_conn: None,
                timeout_idle: None,
            },
        }],
    }
}

fn default_options() -> GlobalOptions {
    GlobalOptions {
        dry_run: false,
        max_copies: 4,
        retries_copy: 1,
        retries_list: 1,
        timeout_conn: 2,
        timeout_idle: 10,
        verbosity: Verbosity::Error,
        keep_tmp_days: 2,
        keep_bak_days: 90,
        keep_del_days: 30,
    }
}

fn build_subject() -> Arc<dyn RunController> {
    let transport = transport::new();
    let output = output::new();
    let snapshot = snapshot::new(transport.clone());
    let copyqueue = copyqueue::new(transport.clone(), output.clone());
    let syncengine =
        syncengine::new(copyqueue.clone(), output.clone(), snapshot.clone(), transport.clone());
    let cli = cli::new();
    runcontroller::new(cli, transport, snapshot, syncengine, copyqueue, output)
}

// 006.1 + 006.2: connection attempts are started for all peers (concurrently)
// and when fewer than two succeed the run exits 1.
#[test]
fn fewer_than_two_reachable_exits_1() {
    let subject = build_subject();
    let config = RunConfig {
        peers: vec![
            make_peer(PeerRole::Normal, "sftp://127.0.0.1:1/peer_a".to_string()),
            make_peer(PeerRole::Normal, "sftp://127.0.0.1:1/peer_b".to_string()),
        ],
        excludes: vec![],
        options: default_options(),
    };
    let outcome = subject.run(config);
    assert_eq!(outcome.exit_code, 1);
}

// 006.3: when the designated canon peer is unreachable the run exits 1 even
// though the remaining two normal peers are reachable.
#[test]
fn canon_peer_unreachable_exits_1() {
    let d1 = fresh_dir("canon_p1");
    let d2 = fresh_dir("canon_p2");
    let subject = build_subject();
    let config = RunConfig {
        peers: vec![
            make_peer(PeerRole::Canon, "sftp://127.0.0.1:1/canon".to_string()),
            make_peer(PeerRole::Normal, local_url(&d1)),
            make_peer(PeerRole::Normal, local_url(&d2)),
        ],
        excludes: vec![],
        options: default_options(),
    };
    let outcome = subject.run(config);
    assert_eq!(outcome.exit_code, 1);
}

// 006.4 + 006.5: when no reachable peer has snapshot data and no canon peer
// is designated, the run exits 1 with the exact first-sync advisory message.
#[test]
fn no_snapshot_no_canon_exits_1_with_first_sync_message() {
    let d1 = fresh_dir("fsync_p1");
    let d2 = fresh_dir("fsync_p2");
    let subject = build_subject();
    let config = RunConfig {
        peers: vec![
            make_peer(PeerRole::Normal, local_url(&d1)),
            make_peer(PeerRole::Normal, local_url(&d2)),
        ],
        excludes: vec![],
        options: default_options(),
    };
    let outcome = subject.run(config);
    assert_eq!(outcome.exit_code, 1);
    assert_eq!(
        outcome.message.as_deref(),
        Some("First sync? Mark the authoritative peer with a leading +"),
    );
}

// 006.6 + 006.7: when, after auto-subordination of snapshotless peers, no
// contributing peer is reachable, the run exits 1 with the exact advisory
// message.  A prior successful canon run seeds snapshot data on both peers
// so the 006.4 gate does not fire; marking both peers explicitly subordinate
// in the second run leaves no contributing peer.
#[test]
fn no_contributing_peer_after_subordination_exits_1_with_message() {
    let d1 = fresh_dir("nocontrib_p1");
    let d2 = fresh_dir("nocontrib_p2");
    std::fs::write(d1.join("seed.txt"), b"seed").unwrap();
    {
        let subject = build_subject();
        let seed_outcome = subject.run(RunConfig {
            peers: vec![
                make_peer(PeerRole::Canon, local_url(&d1)),
                make_peer(PeerRole::Normal, local_url(&d2)),
            ],
            excludes: vec![],
            options: default_options(),
        });
        assert_eq!(seed_outcome.exit_code, 0, "seed run must succeed");
    }
    let subject = build_subject();
    let outcome = subject.run(RunConfig {
        peers: vec![
            make_peer(PeerRole::Subordinate, local_url(&d1)),
            make_peer(PeerRole::Subordinate, local_url(&d2)),
        ],
        excludes: vec![],
        options: default_options(),
    });
    assert_eq!(outcome.exit_code, 1);
    assert_eq!(
        outcome.message.as_deref(),
        Some("No contributing peer reachable - cannot make sync decisions"),
    );
}

// 006.8 + 006.9 + 006.10 + 006.11: a run that completes all phases exits 0;
// all enqueued copies finish before exit (006.9) and updated snapshots are
// written back to peers before exit (006.10).  The copy of hello.txt to peer2
// also exercises the interleaved traversal-and-copy path (006.8): the
// implementation must not defer all copies until the entire tree is scanned.
#[test]
fn normal_run_completes_all_phases_exits_0() {
    let d1 = fresh_dir("normal_p1");
    let d2 = fresh_dir("normal_p2");
    std::fs::write(d1.join("hello.txt"), b"kitchensync").unwrap();
    let subject = build_subject();
    let config = RunConfig {
        peers: vec![
            make_peer(PeerRole::Canon, local_url(&d1)),
            make_peer(PeerRole::Normal, local_url(&d2)),
        ],
        excludes: vec![],
        options: default_options(),
    };
    let outcome = subject.run(config);
    assert_eq!(outcome.exit_code, 0);
    assert!(
        d2.join("hello.txt").exists(),
        "copy must complete before run exits (006.9)"
    );
    assert!(
        d1.join(".kitchensync").join("snapshot.db").exists(),
        "snapshot written back to peer1 before exit (006.10)"
    );
    assert!(
        d2.join(".kitchensync").join("snapshot.db").exists(),
        "snapshot written back to peer2 before exit (006.10)"
    );
}

// 006.12 + 006.13: an unreachable peer is excluded entirely from the run;
// the two reachable peers satisfy all gates so the run exits 0.  The
// unreachable peer's data is never touched because it never joins the run.
#[test]
fn unreachable_peer_excluded_run_exits_0() {
    let d1 = fresh_dir("excl_p1");
    let d2 = fresh_dir("excl_p2");
    let subject = build_subject();
    let config = RunConfig {
        peers: vec![
            make_peer(PeerRole::Canon, local_url(&d1)),
            make_peer(PeerRole::Normal, local_url(&d2)),
            make_peer(PeerRole::Normal, "sftp://127.0.0.1:1/unreachable".to_string()),
        ],
        excludes: vec![],
        options: default_options(),
    };
    let outcome = subject.run(config);
    assert_eq!(outcome.exit_code, 0);
}

// Dry-run: the run is read like a normal run and, with both peers reachable and
// a canon designated, passes the gates and exits 0 -- but every peer-mutating
// step is suppressed.  Compared against normal_run above, no copy is applied to
// peer2 and no snapshot is written back to either peer.
#[test]
fn dry_run_suppresses_copies_and_writeback() {
    let d1 = fresh_dir("dry_p1");
    let d2 = fresh_dir("dry_p2");
    std::fs::write(d1.join("hello.txt"), b"kitchensync").unwrap();
    let subject = build_subject();
    let config = RunConfig {
        peers: vec![
            make_peer(PeerRole::Canon, local_url(&d1)),
            make_peer(PeerRole::Normal, local_url(&d2)),
        ],
        excludes: vec![],
        options: GlobalOptions {
            dry_run: true,
            ..default_options()
        },
    };
    let outcome = subject.run(config);
    assert_eq!(outcome.exit_code, 0);
    assert!(
        !d2.join("hello.txt").exists(),
        "dry-run must not apply copies"
    );
    assert!(
        !d1.join(".kitchensync").join("snapshot.db").exists(),
        "dry-run must not write snapshots back to peer1"
    );
    assert!(
        !d2.join(".kitchensync").join("snapshot.db").exists(),
        "dry-run must not write snapshots back to peer2"
    );
}
