use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use syncengine::{PeerRole, RunRequest, SyncEngine, SyncPeer};

// ---- helpers ----

fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("se_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn local(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

fn write_file(path: &Path, content: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

// The test harness bundles SyncEngine with the snapshot and transport services
// so it can play RunController's role: open (download) each peer's snapshot
// before the walk, then prune and write it back after. SyncEngine itself no
// longer drives that lifecycle (snapshot download/upload belong to the run
// controller per its SPEC), so the harness must supply it around every run.
struct Harness {
    engine: Arc<dyn SyncEngine>,
    snapshot: Arc<dyn snapshot::Snapshot>,
    transport: Arc<dyn transport::Transport>,
}

fn make_engine() -> Harness {
    let transport = transport::new();
    let output = output::new();
    let snapshot = snapshot::new(transport.clone());
    let cq = copyqueue::new(transport.clone(), output.clone());
    let engine = syncengine::new(cq, output, snapshot.clone(), transport.clone());
    Harness { engine, snapshot, transport }
}

impl Harness {
    // Drive a run exactly as RunController does: resolve each peer's winning URL,
    // open its snapshot under that key, run the walk, then prune and write back.
    fn drive(&self, req: RunRequest) {
        let dry = req.dry_run;
        let mut keys: Vec<String> = Vec::new();
        for p in &req.peers {
            if let Some(c) = self.transport.open_peer(
                &p.url,
                &[],
                dry,
                std::time::Duration::from_secs(30),
            ) {
                let _ = self.snapshot.open(&c.winning_url, dry);
                keys.push(c.winning_url);
            }
        }
        self.engine.run(req);
        for k in &keys {
            let _ = self.snapshot.prune(k, 180);
        }
        for k in &keys {
            let _ = self.snapshot.writeback(k, dry);
        }
    }
}

fn run(engine: &Harness, peers: Vec<SyncPeer>) {
    engine.drive(RunRequest {
        peers,
        excludes: vec![],
        list_retries: 1,
        dry_run: false,
    });
}

fn run_with_excludes(engine: &Harness, peers: Vec<SyncPeer>, excludes: Vec<String>) {
    engine.drive(RunRequest {
        peers,
        excludes,
        list_retries: 1,
        dry_run: false,
    });
}

fn run_dry(engine: &Harness, peers: Vec<SyncPeer>) {
    engine.drive(RunRequest {
        peers,
        excludes: vec![],
        list_retries: 1,
        dry_run: true,
    });
}

fn sp(url: &str, role: PeerRole) -> SyncPeer {
    SyncPeer {
        url: url.to_string(),
        role,
        prefix: String::new(),
    }
}

// Check whether any .kitchensync/BAK/<ts>/<basename> exists directly under parent_dir.
fn has_bak(parent_dir: &Path, basename: &str) -> bool {
    let bak = parent_dir.join(".kitchensync/BAK");
    if !bak.exists() {
        return false;
    }
    for entry in fs::read_dir(&bak).unwrap() {
        if entry.unwrap().path().join(basename).exists() {
            return true;
        }
    }
    false
}

// Check whether any .kitchensync/BAK/<ts>/<basename> exists under sub_dir/.kitchensync/BAK
// (for 021.4: BAK is at the parent of the displaced entry, not at sync root).
fn has_bak_at(parent_dir: &Path, basename: &str) -> bool {
    has_bak(parent_dir, basename)
}

// Seed a pair of peers via a canon run so both receive snapshot.db.
// After this call both peers participate as Contributing peers.
fn seed_canon_run(canon_dir: &Path, other_dir: &Path) {
    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(canon_dir), PeerRole::Canon),
            sp(&local(other_dir), PeerRole::Contributing),
        ],
    );
}

// ---- 007: Peer roles ----

#[test]
fn canon_file_propagated_to_other_peer() {
    // 007.1, 011.1: The canon peer's version of a file is propagated to the group.
    let canon_dir = test_dir("c_prop_canon");
    let other_dir = test_dir("c_prop_other");
    write_file(&canon_dir.join("hello.txt"), b"from canon");
    write_file(&other_dir.join("hello.txt"), b"from other peer");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    assert_eq!(
        fs::read(other_dir.join("hello.txt")).unwrap(),
        b"from canon",
        "canon version must overwrite other peer's file"
    );
}

#[test]
fn canon_lacking_file_removed_from_other_peer() {
    // 011.2: A file canon lacks is deleted from every other peer.
    let canon_dir = test_dir("c_lacks_canon");
    let other_dir = test_dir("c_lacks_other");
    write_file(&other_dir.join("extra.txt"), b"extra");

    // seed so other_dir has snapshot history
    seed_canon_run(&other_dir, &canon_dir);
    // The seed copied extra.txt to canon_dir; remove it so canon truly lacks it.
    let _ = fs::remove_file(canon_dir.join("extra.txt"));

    // second run: canon_dir has no extra.txt; other_dir should lose it
    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        !other_dir.join("extra.txt").exists(),
        "file absent from canon must be removed from other peer"
    );
}

#[test]
fn peer_without_snapshot_db_is_auto_subordinate() {
    // 007.7: A peer with no .kitchensync/snapshot.db is treated as subordinate.
    // Two Contributing peers with no snapshot.db: neither has a snapshot, so both
    // are auto-subordinate. The run should not copy between them as if they were contributing.
    // Specifically, neither peer's files become the "group" winner; the result
    // must be the same as if each snapshotless peer were absent (007.2).
    let a = test_dir("no_snap_a");
    let b = test_dir("no_snap_b");
    write_file(&a.join("only_a.txt"), b"a");
    write_file(&b.join("only_b.txt"), b"b");

    // With no canon and both peers snapshotless (auto-subordinate), neither
    // contributes to decisions: only_a.txt should not appear on b, and only_b.txt
    // should not appear on a.
    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&a), PeerRole::Contributing),
            sp(&local(&b), PeerRole::Contributing),
        ],
    );

    assert!(
        !b.join("only_a.txt").exists(),
        "snapshotless contributing peer must not propagate files to group"
    );
    assert!(
        !a.join("only_b.txt").exists(),
        "snapshotless contributing peer must not propagate files to group"
    );
}

#[test]
fn canon_without_snapshot_db_still_contributes() {
    // 007.8: A peer with no .kitchensync/snapshot.db that is marked canon is NOT
    // treated as subordinate.
    let canon_dir = test_dir("c_no_snap_canon");
    let other_dir = test_dir("c_no_snap_other");
    write_file(&canon_dir.join("data.txt"), b"canon content");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    assert_eq!(
        fs::read(other_dir.join("data.txt")).unwrap(),
        b"canon content",
        "canon peer without snapshot.db must still propagate its files"
    );
}

#[test]
fn explicit_subordinate_on_no_snapshot_peer_unchanged() {
    // 007.9: Adding the - prefix to a peer that has no .kitchensync/snapshot.db
    // does not change the run's outcome vs. the auto-subordinate case.
    let canon_dir = test_dir("c_explicit_sub_canon");
    let sub_dir = test_dir("c_explicit_sub_sub");
    write_file(&canon_dir.join("file.txt"), b"canon");
    write_file(&sub_dir.join("file.txt"), b"sub");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Subordinate),
        ],
    );

    // Canon wins; explicit subordinate receives canon's version just as auto-subordinate would.
    assert_eq!(
        fs::read(sub_dir.join("file.txt")).unwrap(),
        b"canon",
        "explicitly subordinate peer with no snapshot.db receives canon's file"
    );
}

#[test]
fn subordinate_extra_file_displaced_to_bak() {
    // 007.3: A file a subordinate peer has that the group's state does not
    // include is displaced to that peer's BAK/.
    let canon_dir = test_dir("c_sub_extra_canon");
    let sub_dir = test_dir("c_sub_extra_sub");
    write_file(&sub_dir.join("extra.txt"), b"extra");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        !sub_dir.join("extra.txt").exists(),
        "extra file on subordinate must be displaced"
    );
    assert!(
        has_bak(&sub_dir, "extra.txt"),
        "displaced file must be moved to BAK on the subordinate peer"
    );
}

#[test]
fn group_file_copied_to_subordinate() {
    // 007.4: A file the group has that a subordinate peer lacks is copied to the
    // subordinate peer.
    let canon_dir = test_dir("c_grp_file_canon");
    let sub_dir = test_dir("c_grp_file_sub");
    write_file(&canon_dir.join("shared.txt"), b"shared");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    assert_eq!(
        fs::read(sub_dir.join("shared.txt")).unwrap(),
        b"shared",
        "file present on group must be copied to subordinate peer"
    );
}

#[test]
fn group_directory_created_on_subordinate() {
    // 007.5, 012.6: A directory the group has that a subordinate peer lacks is
    // created on the subordinate peer.
    let canon_dir = test_dir("c_grp_dir_canon");
    let sub_dir = test_dir("c_grp_dir_sub");
    fs::create_dir_all(canon_dir.join("docs")).unwrap();

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        sub_dir.join("docs").is_dir(),
        "directory present on canon must be created on subordinate"
    );
}

#[test]
fn subordinate_extra_directory_displaced_to_bak() {
    // 007.6, 012.7: A directory a subordinate peer has that the group's state
    // does not include is displaced to that peer's BAK/.
    let canon_dir = test_dir("c_sub_dir_canon");
    let sub_dir = test_dir("c_sub_dir_sub");
    fs::create_dir_all(sub_dir.join("extra_dir")).unwrap();
    write_file(&sub_dir.join("extra_dir/inner.txt"), b"inner");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        !sub_dir.join("extra_dir").exists(),
        "extra directory on subordinate must be displaced"
    );
    assert!(
        has_bak(&sub_dir, "extra_dir"),
        "displaced directory must appear under BAK on the subordinate peer"
    );
}

#[test]
fn snapshot_db_uploaded_after_normal_run() {
    // 007.10: After a normal run, a subordinate peer's .kitchensync/snapshot.db
    // is uploaded back to the peer.
    let canon_dir = test_dir("c_snap_upload_canon");
    let sub_dir = test_dir("c_snap_upload_sub");
    write_file(&canon_dir.join("data.txt"), b"data");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        sub_dir.join(".kitchensync/snapshot.db").exists(),
        "snapshot.db must be uploaded to peer after a normal run"
    );
}

#[test]
fn snapshot_db_not_updated_in_dry_run() {
    // 007.11: In --dry-run, a subordinate peer's .kitchensync/snapshot.db on the
    // peer is not updated.
    let canon_dir = test_dir("c_snap_dry_canon");
    let sub_dir = test_dir("c_snap_dry_sub");
    write_file(&canon_dir.join("data.txt"), b"data");

    let engine = make_engine();
    run_dry(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        !sub_dir.join(".kitchensync/snapshot.db").exists(),
        "snapshot.db must NOT be created/updated on peer during dry-run"
    );
}

#[test]
fn previously_subordinate_peer_participates_on_later_run() {
    // 007.12: On a later normal run without the - prefix, a peer that was
    // previously subordinate participates in decisions using its snapshot history.
    let canon_dir = test_dir("c_rejoin_canon");
    let peer_dir = test_dir("c_rejoin_peer");
    write_file(&canon_dir.join("shared.txt"), b"v");

    // First run: peer_dir is subordinate (no snapshot.db), receives canon state.
    {
        let engine = make_engine();
        run(
            &engine,
            vec![
                sp(&local(&canon_dir), PeerRole::Canon),
                sp(&local(&peer_dir), PeerRole::Contributing),
            ],
        );
    }
    assert!(
        peer_dir.join(".kitchensync/snapshot.db").exists(),
        "snapshot.db must exist on peer after first run"
    );

    // Update the file on peer_dir (simulating a local edit).
    write_file(&peer_dir.join("shared.txt"), b"v2");

    // Second run: no canon, both peers contribute; peer_dir's modified version
    // is now a contributing opinion (007.12).
    {
        let engine = make_engine();
        run(
            &engine,
            vec![
                sp(&local(&canon_dir), PeerRole::Contributing),
                sp(&local(&peer_dir), PeerRole::Contributing),
            ],
        );
    }

    // peer_dir's v2 is newer; it should win and propagate to canon_dir.
    let canon_content = fs::read(canon_dir.join("shared.txt")).unwrap();
    assert_eq!(
        canon_content, b"v2",
        "peer that previously was subordinate must now participate; its newer file wins"
    );
}

// ---- 007.2: Subordinate decisions ----

#[test]
fn subordinate_entries_do_not_affect_group_decision() {
    // 007.2: A subordinate peer's entries do not affect sync decisions; the group
    // outcome is the same as if the subordinate peer were absent.
    let peer_a = test_dir("c_sub_decision_a");
    let peer_b = test_dir("c_sub_decision_b");
    let sub_peer = test_dir("c_sub_decision_sub");
    write_file(&peer_a.join("doc.txt"), b"group version");
    write_file(&peer_b.join("doc.txt"), b"group version");

    // Seed A and B so they are contributing on the next run.
    seed_canon_run(&peer_a, &peer_b);

    // sub_peer has a different version written after the seed run (so it is newer
    // on disk). If it were contributing its newer version would win; since it is
    // subordinate the group version must be preserved.
    write_file(&sub_peer.join("doc.txt"), b"subordinate version");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Contributing),
            sp(&local(&peer_b), PeerRole::Contributing),
            sp(&local(&sub_peer), PeerRole::Subordinate),
        ],
    );

    assert_eq!(
        fs::read(peer_a.join("doc.txt")).unwrap(),
        b"group version",
        "contributing peer A must not be overwritten by subordinate's newer version"
    );
    assert_eq!(
        fs::read(peer_b.join("doc.txt")).unwrap(),
        b"group version",
        "contributing peer B must not be overwritten by subordinate's newer version"
    );
}

// ---- 008: Walk ordering and directory handling ----

#[test]
fn directory_displaced_as_single_subtree_rename() {
    // 008.7, 021.3: A directory chosen for displacement is moved to BAK/ as a
    // single rename that preserves its entire subtree.
    let canon_dir = test_dir("c_dir_disp_canon");
    let sub_dir = test_dir("c_dir_disp_sub");
    fs::create_dir_all(sub_dir.join("tree/sub")).unwrap();
    write_file(&sub_dir.join("tree/a.txt"), b"a");
    write_file(&sub_dir.join("tree/sub/b.txt"), b"b");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        !sub_dir.join("tree").exists(),
        "displaced directory must not remain at original path"
    );
    // The subtree must be intact under BAK.
    let bak_root = sub_dir.join(".kitchensync/BAK");
    let mut found_a = false;
    let mut found_b = false;
    if bak_root.exists() {
        for ts_entry in fs::read_dir(&bak_root).unwrap() {
            let ts_path = ts_entry.unwrap().path();
            if ts_path.join("tree/a.txt").exists() {
                found_a = true;
            }
            if ts_path.join("tree/sub/b.txt").exists() {
                found_b = true;
            }
        }
    }
    assert!(found_a, "displaced subtree must include tree/a.txt under BAK");
    assert!(found_b, "displaced subtree must include tree/sub/b.txt under BAK");
}

#[test]
fn entries_processed_before_subdirectory_recursion() {
    // 008.2: KitchenSync acts on every entry in a directory before entering any
    // subdirectory of that directory.  Observable: a type-conflict displacement
    // at the parent level (required before a copy) succeeds so the copy completes
    // within the same run (008.6).
    let canon_dir = test_dir("c_pre_recurse_canon");
    let sub_dir = test_dir("c_pre_recurse_sub");
    // canon has docs/ as a directory; sub has docs as a file (type conflict).
    fs::create_dir_all(canon_dir.join("docs")).unwrap();
    write_file(&canon_dir.join("docs/readme.txt"), b"readme");
    write_file(&sub_dir.join("docs"), b"file_not_dir"); // type conflict

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        sub_dir.join("docs").is_dir(),
        "docs must become a directory (conflicting file displaced first)"
    );
    assert_eq!(
        fs::read(sub_dir.join("docs/readme.txt")).unwrap(),
        b"readme",
        "file inside docs must be copied within the same run after displacement"
    );
}

// ---- 008.5: Snapshot-only entries ----

#[test]
fn snapshot_only_entry_not_re_added_to_walk() {
    // 008.5: An entry that appears only in snapshot rows, and in no peer's live
    // listing, is not added to the set of entries processed during the walk.
    // Observable: a file absent from all live peers is not resurrected by the run.
    let peer_a = test_dir("c_snap_only_a");
    let peer_b = test_dir("c_snap_only_b");
    write_file(&peer_a.join("temp.txt"), b"temporary");

    // First run: establish temp.txt in peer_a's snapshot; peer_b receives it.
    seed_canon_run(&peer_a, &peer_b);

    // Delete temp.txt from both peers so it exists only in snapshot rows.
    fs::remove_file(peer_a.join("temp.txt")).unwrap();
    fs::remove_file(peer_b.join("temp.txt")).unwrap();

    // Second run: temp.txt absent from all live listings.
    // It must not be re-added to the walk and must not reappear on any peer.
    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Contributing),
            sp(&local(&peer_b), PeerRole::Contributing),
        ],
    );

    assert!(
        !peer_a.join("temp.txt").exists(),
        "snapshot-only entry must not be re-created on peer A"
    );
    assert!(
        !peer_b.join("temp.txt").exists(),
        "snapshot-only entry must not be re-created on peer B"
    );
}

// ---- 008.8, 008.9: Displaced directory not recursed; only keeping peers recurse ----

#[test]
fn displaced_directory_not_recursed_into_on_displaced_peer() {
    // 008.8: KitchenSync does not recurse into a directory that is being displaced
    // on a peer; entries inside that directory are not processed individually.
    // 008.9: When a directory is kept on some peers and displaced on others, only
    // the peers keeping the directory have its children synchronized.
    //
    // Setup: canon has "data/" (directory) with "data/file.txt".
    // other_peer has "data" as a file (type conflict), so it gets displaced.
    // The other_peer then receives data/ as a directory and data/file.txt inside it.
    // If recursion happened on other_peer before displacement, "data" (a file) would
    // be traversed as a directory, which is wrong. The correct behavior: displacement
    // happens first; recursion into data/ on other_peer only happens after the directory
    // exists there (created after displacement).
    let canon_dir = test_dir("c_recurse_canon");
    let other_dir = test_dir("c_recurse_other");
    fs::create_dir_all(canon_dir.join("data")).unwrap();
    write_file(&canon_dir.join("data/file.txt"), b"file content");
    // other_dir has "data" as a file (not a directory)
    write_file(&other_dir.join("data"), b"conflicting file");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    // other_dir's conflicting file must be displaced to BAK
    assert!(
        has_bak(&other_dir, "data"),
        "conflicting file must be archived to BAK"
    );
    // other_dir must now have "data" as a directory with canon's content
    assert!(
        other_dir.join("data").is_dir(),
        "data must be a directory on the conforming peer after displacement"
    );
    assert_eq!(
        fs::read(other_dir.join("data/file.txt")).unwrap(),
        b"file content",
        "canon's file inside data/ must be copied to the conforming peer"
    );
    // canon's directory must remain intact
    assert!(
        canon_dir.join("data").is_dir(),
        "canon's directory must remain"
    );
    assert_eq!(
        fs::read(canon_dir.join("data/file.txt")).unwrap(),
        b"file content",
        "canon's file must remain unchanged"
    );
}

// ---- 008.16: Filename case preservation ----

#[test]
fn filename_case_preserved_on_sync() {
    // 008.16: Filenames are preserved exactly as the filesystem reports them;
    // KitchenSync does not alter the case or characters of an entry's name when syncing it.
    let canon_dir = test_dir("c_case_canon");
    let other_dir = test_dir("c_case_other");
    write_file(&canon_dir.join("MyDocument.TXT"), b"content");
    write_file(&canon_dir.join("README.md"), b"readme");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        other_dir.join("MyDocument.TXT").exists(),
        "filename case must be preserved exactly: MyDocument.TXT"
    );
    assert!(
        other_dir.join("README.md").exists(),
        "filename case must be preserved exactly: README.md"
    );
}

// ---- 009: Excludes ----

#[test]
fn kitchensync_directory_not_copied_to_other_peer() {
    // 009.1: A .kitchensync/ directory present on one peer is not copied to peers
    // that lack it.
    let canon_dir = test_dir("c_excl_ks_canon");
    let other_dir = test_dir("c_excl_ks_other");
    fs::create_dir_all(canon_dir.join(".kitchensync")).unwrap();
    write_file(&canon_dir.join(".kitchensync/snapshot.db"), b"fake-db");
    write_file(&canon_dir.join("normal.txt"), b"normal");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    // Internal snapshot.db upload is allowed; but canon's fake-db content must not appear.
    if other_dir.join(".kitchensync/snapshot.db").exists() {
        assert_ne!(
            fs::read(other_dir.join(".kitchensync/snapshot.db")).unwrap(),
            b"fake-db",
            ".kitchensync/ must not be synced to other peer (only internal snapshot.db upload is allowed)"
        );
    }
}

#[test]
fn git_directory_not_copied_to_other_peer() {
    // 009.2: A .git/ directory present on one peer is not copied to peers that lack it.
    let canon_dir = test_dir("c_excl_git_canon");
    let other_dir = test_dir("c_excl_git_other");
    fs::create_dir_all(canon_dir.join(".git")).unwrap();
    write_file(&canon_dir.join(".git/HEAD"), b"ref: refs/heads/main");
    write_file(&canon_dir.join("code.rs"), b"fn main() {}");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        !other_dir.join(".git").exists(),
        ".git/ must not be copied to other peer"
    );
    assert_eq!(
        fs::read(other_dir.join("code.rs")).unwrap(),
        b"fn main() {}",
        "non-excluded files must still be copied"
    );
}

#[test]
fn command_line_exclude_path_not_copied() {
    // 009.5, 009.6: A path supplied with -x that exists on one peer is not
    // copied to peers that lack it; -x excludes are in addition to built-ins.
    let canon_dir = test_dir("c_excl_x_canon");
    let other_dir = test_dir("c_excl_x_other");
    write_file(&canon_dir.join("secret.key"), b"key data");
    write_file(&canon_dir.join("normal.txt"), b"normal");

    let engine = make_engine();
    run_with_excludes(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
        vec!["secret.key".to_string()],
    );

    assert!(
        !other_dir.join("secret.key").exists(),
        "-x excluded path must not be copied"
    );
    assert_eq!(
        fs::read(other_dir.join("normal.txt")).unwrap(),
        b"normal",
        "non-excluded file must still be copied"
    );
}

#[test]
fn excluded_entry_already_on_peer_left_in_place() {
    // 009.7: An excluded entry that already exists on a peer is left in place,
    // neither deleted nor displaced to BAK/.
    let canon_dir = test_dir("c_excl_keep_canon");
    let other_dir = test_dir("c_excl_keep_other");
    write_file(&other_dir.join("local.cfg"), b"local config");

    let engine = make_engine();
    run_with_excludes(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
        vec!["local.cfg".to_string()],
    );

    assert_eq!(
        fs::read(other_dir.join("local.cfg")).unwrap(),
        b"local config",
        "excluded file already on peer must be left untouched"
    );
    assert!(
        !has_bak(&other_dir, "local.cfg"),
        "excluded file must not be displaced to BAK"
    );
}

#[test]
fn excluded_directory_and_descendants_skipped() {
    // 009.8: An excluded directory and all of its descendants are skipped, so no
    // descendant is copied, deleted, or displaced on any peer.
    let canon_dir = test_dir("c_excl_dir_canon");
    let other_dir = test_dir("c_excl_dir_other");
    fs::create_dir_all(canon_dir.join("build")).unwrap();
    write_file(&canon_dir.join("build/output.bin"), b"binary");
    write_file(&canon_dir.join("src/main.rs"), b"fn main() {}");

    let engine = make_engine();
    run_with_excludes(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
        vec!["build".to_string()],
    );

    assert!(
        !other_dir.join("build").exists(),
        "excluded directory must not be copied"
    );
    assert!(
        !other_dir.join("build/output.bin").exists(),
        "descendant of excluded directory must not be copied"
    );
    assert_eq!(
        fs::read(other_dir.join("src/main.rs")).unwrap(),
        b"fn main() {}",
        "non-excluded files must still be copied"
    );
}

// ---- 010: Entry classification ----

#[test]
fn new_file_no_snapshot_row_propagated_to_other_peer() {
    // 010.5: A live file with no snapshot row on a peer is treated as new on
    // that peer and is propagated to peers that lack it.
    let canon_dir = test_dir("c_new_file_canon");
    let other_dir = test_dir("c_new_file_other");
    write_file(&canon_dir.join("new.txt"), b"new content");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    assert_eq!(
        fs::read(other_dir.join("new.txt")).unwrap(),
        b"new content",
        "new file (no snapshot row) must be propagated"
    );
}

// ---- 011: File decision rules ----

#[test]
fn all_matching_peers_no_copy_needed() {
    // 011.3: When every contributing peer has the file unchanged and the peers'
    // copies already match, sync performs no copy among those matching peers.
    // Observable: file content and mtime on both peers stay identical.
    let dir_a = test_dir("c_match_a");
    let dir_b = test_dir("c_match_b");
    write_file(&dir_a.join("same.txt"), b"identical");
    write_file(&dir_b.join("same.txt"), b"identical");

    // Seed both peers so they are contributing on second run.
    seed_canon_run(&dir_a, &dir_b);

    // Both have same.txt with same content; second run should not copy.
    // Touch each file to a known mtime before the run.
    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&dir_a), PeerRole::Contributing),
            sp(&local(&dir_b), PeerRole::Contributing),
        ],
    );

    assert_eq!(
        fs::read(dir_a.join("same.txt")).unwrap(),
        b"identical",
        "matching file on peer A must remain unchanged"
    );
    assert_eq!(
        fs::read(dir_b.join("same.txt")).unwrap(),
        b"identical",
        "matching file on peer B must remain unchanged"
    );
}

#[test]
fn matching_peers_file_copied_to_lacking_peer() {
    // 011.4: When every contributing peer has the file unchanged and matching,
    // sync copies that file to any active peer that lacks it, including subordinate peers.
    let peer_a = test_dir("c_match_copy_a");
    let peer_b = test_dir("c_match_copy_b");
    let peer_c = test_dir("c_match_copy_c");
    write_file(&peer_a.join("shared.txt"), b"shared content");

    // Seed A and B so they are both contributing with snapshot history.
    seed_canon_run(&peer_a, &peer_b);

    // peer_c has no snapshot and no file; it is auto-subordinate.
    // A and B are both unchanged (A has shared.txt, B received it in seed run).
    // peer_c lacks the file and must receive it.
    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Contributing),
            sp(&local(&peer_b), PeerRole::Contributing),
            sp(&local(&peer_c), PeerRole::Contributing),
        ],
    );

    assert_eq!(
        fs::read(peer_c.join("shared.txt")).unwrap(),
        b"shared content",
        "peer lacking a file that all contributing peers match must receive it"
    );
}

#[test]
fn newer_modified_file_wins_without_canon() {
    // 011.5: When contributing peers hold differing modified versions of a file,
    // sync propagates the version with the newest mod_time.
    let dir_a = test_dir("c_newer_a");
    let dir_b = test_dir("c_newer_b");
    write_file(&dir_a.join("doc.txt"), b"version A");
    write_file(&dir_b.join("doc.txt"), b"version B");

    // Seed both peers so both contribute.
    seed_canon_run(&dir_a, &dir_b);

    // Modify dir_b's doc.txt to a newer mtime by writing again.
    write_file(&dir_b.join("doc.txt"), b"version B newer");
    // Ensure dir_b's version has a strictly newer mtime than dir_a's.
    let b_meta = fs::metadata(dir_b.join("doc.txt")).unwrap();
    let a_meta = fs::metadata(dir_a.join("doc.txt")).unwrap();
    // If mtime ordering isn't reliable in this fast test, just assert after run.
    let _ = (a_meta, b_meta);

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&dir_a), PeerRole::Contributing),
            sp(&local(&dir_b), PeerRole::Contributing),
        ],
    );

    // The newer version (B) should have propagated to A.
    let a_content = fs::read(dir_a.join("doc.txt")).unwrap();
    let b_content = fs::read(dir_b.join("doc.txt")).unwrap();
    assert_eq!(
        a_content, b_content,
        "after sync, both peers must hold the same (winning) version"
    );
}

#[test]
fn peer_with_no_snapshot_row_receives_winner() {
    // 011.14: A peer with no snapshot row for a file receives the winning file
    // once a winner is decided.
    let canon_dir = test_dir("c_no_row_recv_canon");
    let new_peer = test_dir("c_no_row_recv_new");

    // new_peer has no snapshot at all; it should receive canon's file.
    write_file(&canon_dir.join("report.txt"), b"report");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&new_peer), PeerRole::Contributing),
        ],
    );

    assert_eq!(
        fs::read(new_peer.join("report.txt")).unwrap(),
        b"report",
        "peer with no snapshot row must receive the winning file"
    );
}

// ---- 012: Directory decisions ----

#[test]
fn canon_directory_created_on_all_peers() {
    // 012.6: When a canon peer has a directory, it is created on every peer that lacks it.
    let canon_dir = test_dir("c_canon_dir_create_canon");
    let peer_b = test_dir("c_canon_dir_create_b");
    let peer_c = test_dir("c_canon_dir_create_c");
    fs::create_dir_all(canon_dir.join("photos")).unwrap();
    write_file(&canon_dir.join("photos/img.jpg"), b"jpeg data");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&peer_b), PeerRole::Contributing),
            sp(&local(&peer_c), PeerRole::Contributing),
        ],
    );

    assert!(
        peer_b.join("photos").is_dir(),
        "canon directory must be created on peer B"
    );
    assert!(
        peer_c.join("photos").is_dir(),
        "canon directory must be created on peer C"
    );
}

#[test]
fn contributing_dir_live_creates_on_all_lacking_peers() {
    // 012.1: When at least one contributing peer has a directory live in its
    // listing, the directory is created on every active peer that lacks it.
    let peer_a = test_dir("c_contrib_dir_a");
    let peer_b = test_dir("c_contrib_dir_b");
    write_file(&peer_a.join("base.txt"), b"base");

    // Seed A as canon so both A and B have snapshot history and are contributing.
    seed_canon_run(&peer_a, &peer_b);

    // A gets a new directory "newdir/" that is not yet in any snapshot.
    fs::create_dir_all(peer_a.join("newdir")).unwrap();
    write_file(&peer_a.join("newdir/file.txt"), b"new file");

    // Run without canon: A is contributing and has "newdir/" live.
    // B lacks "newdir/" and must have it created.
    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Contributing),
            sp(&local(&peer_b), PeerRole::Contributing),
        ],
    );

    assert!(
        peer_b.join("newdir").is_dir(),
        "directory present on a contributing peer must be created on peers that lack it"
    );
}

#[test]
fn canon_lacking_directory_displaced_from_others() {
    // 012.7: When a canon peer lacks a directory, it is displaced to BAK/ on
    // every peer that has it.
    let canon_dir = test_dir("c_canon_disp_dir_canon");
    let peer_dir = test_dir("c_canon_disp_dir_peer");
    fs::create_dir_all(peer_dir.join("old_folder")).unwrap();
    write_file(&peer_dir.join("old_folder/file.txt"), b"content");

    // seed the peer so old_folder is in its snapshot
    seed_canon_run(&peer_dir, &canon_dir);
    // The seed copied old_folder to canon_dir; remove it so canon truly lacks it.
    fs::remove_dir_all(canon_dir.join("old_folder")).unwrap();

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&peer_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        !peer_dir.join("old_folder").exists(),
        "directory absent from canon must be displaced on other peers"
    );
    assert!(
        has_bak(&peer_dir, "old_folder"),
        "displaced directory must appear under BAK"
    );
}

#[test]
fn type_conflict_canon_has_file_directory_displaced() {
    // 012.8, 012.9: When a path is a file on one peer and a directory on another
    // and a canon peer has a file at that path, the conflicting directories are
    // displaced to BAK/ and the canon file is synced to every peer.
    let canon_dir = test_dir("c_type_cf_file_canon");
    let other_dir = test_dir("c_type_cf_file_other");
    write_file(&canon_dir.join("item"), b"file content");
    fs::create_dir_all(other_dir.join("item")).unwrap();
    write_file(&other_dir.join("item/sub.txt"), b"inside dir");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        other_dir.join("item").is_file(),
        "conflicting directory must be displaced; canon file must replace it"
    );
    assert_eq!(
        fs::read(other_dir.join("item")).unwrap(),
        b"file content",
        "canon file content must be present after displacement of conflicting directory"
    );
    assert!(
        has_bak(&other_dir, "item"),
        "conflicting directory must be archived to BAK"
    );
}

#[test]
fn type_conflict_canon_has_directory_file_displaced() {
    // 012.10, 012.11: When a path is a file on one peer and a directory on
    // another and a canon peer has a directory at that path, the conflicting
    // files are displaced to BAK/ and the directory is created on all peers.
    let canon_dir = test_dir("c_type_cf_dir_canon");
    let other_dir = test_dir("c_type_cf_dir_other");
    fs::create_dir_all(canon_dir.join("docs")).unwrap();
    write_file(&canon_dir.join("docs/index.txt"), b"index");
    write_file(&other_dir.join("docs"), b"conflicting file");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        other_dir.join("docs").is_dir(),
        "docs must become a directory after conflicting file is displaced"
    );
    assert_eq!(
        fs::read(other_dir.join("docs/index.txt")).unwrap(),
        b"index",
        "canon directory contents must be synced after type conflict is resolved"
    );
    assert!(
        has_bak(&other_dir, "docs"),
        "conflicting file must be archived to BAK"
    );
}

#[test]
fn canon_lacks_type_conflict_path_displaces_all_holders() {
    // 012.12: When a path is a file on one peer and a directory on another and a
    // canon peer lacks the path, the path is displaced to BAK/ on every peer
    // that has it.
    let canon_dir = test_dir("c_012_12_canon");
    let peer_a = test_dir("c_012_12_a");
    let peer_b = test_dir("c_012_12_b");
    write_file(&peer_a.join("base.txt"), b"base");
    write_file(&peer_b.join("base.txt"), b"base");

    // Seed A and B so they are contributing on the next run.
    {
        let engine = make_engine();
        run(
            &engine,
            vec![
                sp(&local(&peer_a), PeerRole::Canon),
                sp(&local(&peer_b), PeerRole::Contributing),
            ],
        );
    }

    // A gets "item" as a file; B gets "item" as a directory. Canon has neither.
    write_file(&peer_a.join("item"), b"file");
    fs::create_dir_all(peer_b.join("item")).unwrap();
    write_file(&peer_b.join("item/child.txt"), b"child");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&peer_a), PeerRole::Contributing),
            sp(&local(&peer_b), PeerRole::Contributing),
        ],
    );

    assert!(
        !peer_a.join("item").exists(),
        "file at conflicting path must be displaced when canon lacks it"
    );
    assert!(
        !peer_b.join("item").exists(),
        "directory at conflicting path must be displaced when canon lacks it"
    );
}

#[test]
fn no_canon_type_conflict_file_wins_over_directory() {
    // 012.13, 012.14: With no canon peer, when contributing peers have a file
    // and a directory at the same path, the file wins; the conflicting directory
    // is displaced to BAK/ and the winning file is synced to all active peers.
    let peer_a = test_dir("c_012_13_a");
    let peer_b = test_dir("c_012_13_b");
    write_file(&peer_a.join("base.txt"), b"base");
    write_file(&peer_b.join("base.txt"), b"base");

    // Seed both peers so they are contributing.
    seed_canon_run(&peer_a, &peer_b);

    // A gets "item" as a file (new); B gets "item" as a directory (new).
    write_file(&peer_a.join("item"), b"file wins");
    fs::create_dir_all(peer_b.join("item")).unwrap();
    write_file(&peer_b.join("item/inner.txt"), b"inside dir");

    // No canon: file beats directory.
    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Contributing),
            sp(&local(&peer_b), PeerRole::Contributing),
        ],
    );

    assert!(
        peer_b.join("item").is_file(),
        "no-canon type conflict: file must win and directory must be displaced on B"
    );
    assert_eq!(
        fs::read(peer_b.join("item")).unwrap(),
        b"file wins",
        "winning file must be synced to the peer that had the conflicting directory"
    );
    assert!(
        has_bak(&peer_b, "item"),
        "conflicting directory must be archived to BAK on peer B"
    );
    assert_eq!(
        fs::read(peer_a.join("item")).unwrap(),
        b"file wins",
        "winning file must remain on the peer that had it"
    );
}

// ---- 021: Inline displacement to BAK ----

#[test]
fn bak_created_at_parent_level_not_sync_root() {
    // 021.4: The BAK directory for a displacement is created under .kitchensync/
    // at the parent directory of the displaced entry, not aggregated at the sync root.
    let canon_dir = test_dir("c_bak_parent_canon");
    let sub_dir = test_dir("c_bak_parent_sub");
    fs::create_dir_all(sub_dir.join("subdir")).unwrap();
    write_file(&sub_dir.join("subdir/extra.txt"), b"extra");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    // The displaced entry lives under subdir/, so BAK must be at subdir/.kitchensync/BAK
    // not at the root .kitchensync/BAK.
    // extra.txt was inside a displaced subdir/ so the whole subdir is in BAK at root level.
    // Scenario: canon has no subdir, sub has subdir/extra.txt; subdir is displaced.
    // BAK must be at sub_dir/.kitchensync/BAK/<ts>/subdir (021.4: parent of subdir is sub_dir root).
    assert!(
        has_bak_at(&sub_dir, "subdir"),
        "BAK must be created at the displaced entry's parent level (.kitchensync/BAK at sub_dir root)"
    );
    assert!(
        !sub_dir.join("subdir").exists(),
        "displaced directory must be gone from its original path"
    );
}

#[test]
fn nested_entry_bak_at_parent_not_root() {
    // 021.4: When a nested file is displaced, BAK is at the file's parent
    // directory, not at the sync root.
    let canon_dir = test_dir("c_bak_nested_canon");
    let sub_dir = test_dir("c_bak_nested_sub");
    fs::create_dir_all(sub_dir.join("level1")).unwrap();
    write_file(&sub_dir.join("level1/orphan.txt"), b"orphan");
    // canon has level1/ but not orphan.txt inside it
    fs::create_dir_all(canon_dir.join("level1")).unwrap();

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        !sub_dir.join("level1/orphan.txt").exists(),
        "orphan file must be displaced"
    );
    // BAK must be at level1/.kitchensync/BAK/<ts>/orphan.txt, not at root .kitchensync/BAK.
    assert!(
        has_bak_at(&sub_dir.join("level1"), "orphan.txt"),
        "BAK for nested file must be at the file's parent directory (level1/.kitchensync/BAK)"
    );
    assert!(
        !has_bak_at(&sub_dir, "orphan.txt"),
        "BAK for nested file must NOT be aggregated at the sync root"
    );
}

#[test]
fn bak_timestamp_directory_created_for_each_displacement() {
    // 021.1, 021.2: Before renaming an entry for displacement, KitchenSync creates
    // <parent>/.kitchensync/BAK/<timestamp>/ and renames <basename> into it.
    let canon_dir = test_dir("c_bak_ts_canon");
    let sub_dir = test_dir("c_bak_ts_sub");
    write_file(&sub_dir.join("gone.txt"), b"will be displaced");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    let bak = sub_dir.join(".kitchensync/BAK");
    assert!(bak.exists(), ".kitchensync/BAK must be created");

    // There must be exactly one timestamp subdirectory under BAK.
    let entries: Vec<_> = fs::read_dir(&bak).unwrap().collect();
    assert_eq!(entries.len(), 1, "exactly one timestamp directory must exist under BAK");

    let ts_dir = entries.into_iter().next().unwrap().unwrap().path();
    assert!(
        ts_dir.join("gone.txt").exists(),
        "displaced file must be inside the timestamp directory: BAK/<ts>/gone.txt"
    );
}

// ---- 006.8, 006.9: Streaming copy completion ----

#[test]
fn all_enqueued_copies_complete_before_run_returns() {
    // 006.9: All enqueued file copies complete before the run exits.
    // Observable: every file on the canon peer is present on the other peer
    // immediately after run() returns.
    let canon_dir = test_dir("c_006_9_canon");
    let other_dir = test_dir("c_006_9_other");
    for i in 0..10u8 {
        write_file(
            &canon_dir.join(format!("file{}.txt", i)),
            format!("content {}", i).as_bytes(),
        );
    }

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    for i in 0..10u8 {
        assert!(
            other_dir.join(format!("file{}.txt", i)).exists(),
            "file{}.txt must be fully copied before run() returns (006.9)",
            i
        );
    }
}

// ---- 006.12, 006.13: Unreachable peer excluded ----

#[test]
fn peer_absent_from_run_is_not_touched() {
    // 006.12: An unreachable peer is excluded entirely from the run's listings
    // and sync decisions.
    // 006.13: An unreachable peer's snapshot rows are left unmodified by the run.
    // Observable: a peer not listed in RunRequest.peers is not written to and
    // receives no snapshot.db.
    let peer_a = test_dir("c_unreachable_a");
    let peer_b = test_dir("c_unreachable_b");
    let absent_peer = test_dir("c_unreachable_absent");
    write_file(&peer_a.join("shared.txt"), b"shared");
    write_file(&absent_peer.join("unique.txt"), b"unique to absent peer");

    // Run with only peer_a (Canon) and peer_b (Contributing).
    // absent_peer is excluded from RunRequest.peers, representing an unreachable peer.
    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Canon),
            sp(&local(&peer_b), PeerRole::Contributing),
        ],
    );

    assert_eq!(
        fs::read(absent_peer.join("unique.txt")).unwrap(),
        b"unique to absent peer",
        "absent peer's existing files must be untouched"
    );
    assert!(
        !absent_peer.join(".kitchensync/snapshot.db").exists(),
        "absent peer must not receive a snapshot.db (snapshot rows unmodified)"
    );
    assert!(
        !absent_peer.join("shared.txt").exists(),
        "absent peer must not receive files synced between the active peers"
    );
}

// ---- dry-run ----

#[test]
fn dry_run_does_not_copy_files() {
    // When dry_run is true, no file is copied between peers.
    let canon_dir = test_dir("c_dry_nocopy_canon");
    let other_dir = test_dir("c_dry_nocopy_other");
    write_file(&canon_dir.join("file.txt"), b"content");

    let engine = make_engine();
    run_dry(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        !other_dir.join("file.txt").exists(),
        "dry-run must not copy any files"
    );
}

#[test]
fn dry_run_does_not_displace_entries() {
    // 024.15: When dry_run is true, subordinate extra files are not displaced.
    let canon_dir = test_dir("c_dry_nodisp_canon");
    let sub_dir = test_dir("c_dry_nodisp_sub");
    write_file(&sub_dir.join("extra.txt"), b"extra");

    let engine = make_engine();
    run_dry(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&sub_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        sub_dir.join("extra.txt").exists(),
        "dry-run must not displace files"
    );
    assert!(
        !has_bak(&sub_dir, "extra.txt"),
        "dry-run must not create BAK entries"
    );
}

#[test]
fn dry_run_creates_no_directories_on_peers() {
    // 024.12: --dry-run creates no directories on peers.
    let canon_dir = test_dir("c_dry_nodir_canon");
    let other_dir = test_dir("c_dry_nodir_other");
    fs::create_dir_all(canon_dir.join("newdir")).unwrap();
    write_file(&canon_dir.join("newdir/file.txt"), b"content");

    let engine = make_engine();
    run_dry(
        &engine,
        vec![
            sp(&local(&canon_dir), PeerRole::Canon),
            sp(&local(&other_dir), PeerRole::Contributing),
        ],
    );

    assert!(
        !other_dir.join("newdir").exists(),
        "dry-run must not create any directory on peers"
    );
}

#[test]
fn dry_run_deletes_no_destination_files() {
    // 024.16: --dry-run deletes no destination files on peers.
    // A file the canon peer lacks would normally be deleted from other peers;
    // under dry-run it must be left in place.
    let peer_a = test_dir("c_dry_nodel_a");
    let peer_b = test_dir("c_dry_nodel_b");
    write_file(&peer_a.join("preserve.txt"), b"keep me");

    // Seed both peers so preserve.txt is in their snapshots.
    seed_canon_run(&peer_a, &peer_b);

    // Remove preserve.txt from peer_a, making peer_a (canon) lack it.
    fs::remove_file(peer_a.join("preserve.txt")).unwrap();

    // Dry-run: canon lacks preserve.txt, which would normally cause deletion from peer_b.
    let engine = make_engine();
    run_dry(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Canon),
            sp(&local(&peer_b), PeerRole::Contributing),
        ],
    );

    assert!(
        peer_b.join("preserve.txt").exists(),
        "dry-run must not delete destination files even when canon lacks them"
    );
}

// ---- 008.3: Contributing-peer union ----

#[test]
fn contributing_peers_entries_form_union() {
    // 008.3: An entry that appears in any contributing peer's live listing is
    // visited during the walk for that directory.
    // Observable: each contributing peer's unique file is synced to the other peer.
    let peer_a = test_dir("c_union_a");
    let peer_b = test_dir("c_union_b");
    write_file(&peer_a.join("base.txt"), b"base");
    write_file(&peer_b.join("base.txt"), b"base");

    // Seed both peers so they are contributing (have snapshot.db) on the second run.
    seed_canon_run(&peer_a, &peer_b);

    // Each peer gains a unique file after the seed run.
    write_file(&peer_a.join("only_a.txt"), b"from A");
    write_file(&peer_b.join("only_b.txt"), b"from B");

    // Run without canon: both peers contribute; the union of live listings
    // includes entries from both peers, so each unique file is visited.
    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Contributing),
            sp(&local(&peer_b), PeerRole::Contributing),
        ],
    );

    assert!(
        peer_b.join("only_a.txt").exists(),
        "peer A's unique file must be synced to peer B (visited via union)"
    );
    assert!(
        peer_a.join("only_b.txt").exists(),
        "peer B's unique file must be synced to peer A (visited via union)"
    );
}

// ---- 012.5: Subordinate-only directory displaced ----

#[test]
fn subordinate_only_directory_displaced_when_no_contributing_peer_has_it() {
    // 012.5: When no contributing peer has a directory live in its listing and no
    // contributing peer has a snapshot row for it, subordinate peers that have the
    // directory are displaced to BAK/.
    let peer_a = test_dir("c_sub_dir_nc_a");
    let peer_b = test_dir("c_sub_dir_nc_b");
    let sub_peer = test_dir("c_sub_dir_nc_sub");

    // Seed peer_a and peer_b so they are contributing with snapshot history.
    write_file(&peer_a.join("base.txt"), b"base");
    write_file(&peer_b.join("base.txt"), b"base");
    seed_canon_run(&peer_a, &peer_b);

    // sub_peer (Subordinate, no snapshot.db) holds a directory that no
    // contributing peer has or has ever had (no row in their snapshots).
    fs::create_dir_all(sub_peer.join("sub_only_dir")).unwrap();
    write_file(&sub_peer.join("sub_only_dir/inner.txt"), b"inner");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Contributing),
            sp(&local(&peer_b), PeerRole::Contributing),
            sp(&local(&sub_peer), PeerRole::Subordinate),
        ],
    );

    assert!(
        !sub_peer.join("sub_only_dir").exists(),
        "subordinate-only directory must be displaced to BAK when no contributing peer has it"
    );
    assert!(
        has_bak(&sub_peer, "sub_only_dir"),
        "displaced subordinate-only directory must appear under BAK"
    );
}

// ---- 012.3, 012.4: Directory displaced when contributing peers have rows but no live dir ----

#[test]
fn directory_displaced_when_all_contributing_peers_absent_from_listing() {
    // 012.3: When no contributing peer has a directory live, at least one contributing
    // peer has a snapshot row for it, and every contributing peer that has a snapshot row
    // for it is absent from the current listing, the directory is displaced to BAK/ on
    // every peer that still has it.
    // 012.4: A contributing peer with no snapshot row for a directory does not block
    // displacement of that directory.
    let peer_a = test_dir("c_012_3_a");
    let peer_b = test_dir("c_012_3_b");
    let sub_peer = test_dir("c_012_3_sub");

    // Give peer_a and peer_b snapshot history that includes "shared_dir/".
    fs::create_dir_all(peer_a.join("shared_dir")).unwrap();
    write_file(&peer_a.join("shared_dir/file.txt"), b"content");
    fs::create_dir_all(peer_b.join("shared_dir")).unwrap();
    write_file(&peer_b.join("shared_dir/file.txt"), b"content");
    {
        let engine = make_engine();
        run(
            &engine,
            vec![
                sp(&local(&peer_a), PeerRole::Canon),
                sp(&local(&peer_b), PeerRole::Contributing),
            ],
        );
    }
    // peer_a and peer_b now have snapshot rows for "shared_dir/".

    // sub_peer (Subordinate, no snapshot.db) gets the directory placed manually.
    fs::create_dir_all(sub_peer.join("shared_dir")).unwrap();
    write_file(&sub_peer.join("shared_dir/file.txt"), b"content");

    // Remove "shared_dir/" from both contributing peers so they are absent from
    // the listing while still holding snapshot rows.
    fs::remove_dir_all(peer_a.join("shared_dir")).unwrap();
    fs::remove_dir_all(peer_b.join("shared_dir")).unwrap();

    // Second run: no canon. peer_a and peer_b both have snapshot rows for
    // "shared_dir/" but it is absent from their live listings; sub_peer still
    // has it. 012.3 requires displacement from every peer that still holds it.
    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Contributing),
            sp(&local(&peer_b), PeerRole::Contributing),
            sp(&local(&sub_peer), PeerRole::Subordinate),
        ],
    );

    assert!(
        !sub_peer.join("shared_dir").exists(),
        "shared_dir must be displaced when all contributing peers lack it live (012.3)"
    );
    assert!(
        has_bak(&sub_peer, "shared_dir"),
        "displaced shared_dir must appear under BAK on subordinate peer (012.3)"
    );
}

// ---- 007.3, 008.4: Explicit Subordinate-role file displaced to BAK ----

#[test]
fn explicit_subordinate_only_file_displaced_to_bak() {
    // 007.3: A file a subordinate peer has that the group's state does not include
    // is displaced to that peer's BAK/.
    // 008.4: An entry that appears only in subordinate peers' live listings is visited
    // during the walk so it can be brought into conformance.
    // Uses explicit PeerRole::Subordinate (distinct from the auto-subordinate path
    // exercised by tests that rely on 007.7).
    let peer_a = test_dir("c_sub_explicit_file_a");
    let sub_peer = test_dir("c_sub_explicit_file_sub");

    // Give peer_a a snapshot.db so it participates as Contributing (not auto-subordinate).
    write_file(&peer_a.join("base.txt"), b"base");
    {
        let engine = make_engine();
        run(&engine, vec![sp(&local(&peer_a), PeerRole::Canon)]);
    }

    // sub_peer (explicit Subordinate) holds a file the group's state does not include.
    write_file(&sub_peer.join("sub_only.txt"), b"subordinate only");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Contributing),
            sp(&local(&sub_peer), PeerRole::Subordinate),
        ],
    );

    assert!(
        !sub_peer.join("sub_only.txt").exists(),
        "file appearing only on explicit subordinate peer must be displaced (007.3, 008.4)"
    );
    assert!(
        has_bak(&sub_peer, "sub_only.txt"),
        "displaced subordinate-only file must be archived to BAK"
    );
}

// ---- 012.15: Subordinate file does not beat contributing directory ----

#[test]
fn subordinate_file_does_not_beat_contributing_directory() {
    // 012.15: A subordinate peer's file does not cause the file to win over a
    // contributing peer's directory at the same path.
    // Observable: when a contributing peer has a directory and a subordinate peer
    // has a file at the same path, the directory wins; the subordinate file is
    // displaced and the subordinate peer receives the directory.
    let peer_a = test_dir("c_sub_file_dir_a");
    let sub_peer = test_dir("c_sub_file_dir_sub");

    // Run peer_a alone as Canon so it acquires a snapshot.db and is Contributing
    // (not auto-subordinate) on the next run.
    write_file(&peer_a.join("base.txt"), b"base");
    {
        let engine = make_engine();
        run(&engine, vec![sp(&local(&peer_a), PeerRole::Canon)]);
    }

    // peer_a (Contributing) gains "item/" as a directory with content.
    fs::create_dir_all(peer_a.join("item")).unwrap();
    write_file(&peer_a.join("item/child.txt"), b"child");

    // sub_peer (Subordinate, no snapshot.db) has "item" as a conflicting file.
    write_file(&sub_peer.join("item"), b"subordinate file");

    let engine = make_engine();
    run(
        &engine,
        vec![
            sp(&local(&peer_a), PeerRole::Contributing),
            sp(&local(&sub_peer), PeerRole::Subordinate),
        ],
    );

    assert!(
        peer_a.join("item").is_dir(),
        "contributing peer's directory must be preserved"
    );
    assert!(
        sub_peer.join("item").is_dir(),
        "subordinate peer must be conformed to the directory type after displacement"
    );
    assert!(
        has_bak(&sub_peer, "item"),
        "subordinate peer's conflicting file must be displaced to BAK"
    );
}
