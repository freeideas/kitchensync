use snapshot_transfer::{new, PeerFiles, Transfer, TransferError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// ---- Peer file paths as the implementation uses them ----

const LIVE: &str = ".kitchensync/snapshot.db";
const SWAP_OLD: &str = ".kitchensync/SWAP/snapshot.db/old";
const SWAP_NEW: &str = ".kitchensync/SWAP/snapshot.db/new";

// ---- FakePeer: controlled in-memory peer filesystem ----

struct FakePeer {
    files: Mutex<HashMap<String, Vec<u8>>>,
    log: Mutex<Vec<String>>,
    fail_upload: Option<String>,
    fail_rename_src: Option<String>,
}

impl FakePeer {
    fn empty() -> Self {
        FakePeer {
            files: Mutex::new(HashMap::new()),
            log: Mutex::new(Vec::new()),
            fail_upload: None,
            fail_rename_src: None,
        }
    }

    fn with_files(paths: &[&str]) -> Self {
        let mut files = HashMap::new();
        for &p in paths {
            files.insert(p.to_string(), b"peer-content".to_vec());
        }
        FakePeer {
            files: Mutex::new(files),
            log: Mutex::new(Vec::new()),
            fail_upload: None,
            fail_rename_src: None,
        }
    }

    fn failing_upload(mut self, remote: &str) -> Self {
        self.fail_upload = Some(remote.to_string());
        self
    }

    fn failing_rename_of(mut self, src: &str) -> Self {
        self.fail_rename_src = Some(src.to_string());
        self
    }

    fn has(&self, path: &str) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    fn content(&self, path: &str) -> Option<Vec<u8>> {
        self.files.lock().unwrap().get(path).cloned()
    }

    fn ops(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl PeerFiles for FakePeer {
    fn exists(&self, path: &str) -> Result<bool, TransferError> {
        Ok(self.files.lock().unwrap().contains_key(path))
    }

    fn download(&self, remote: &str, local: &Path) -> Result<(), TransferError> {
        self.log.lock().unwrap().push(format!("download:{}", remote));
        let content_opt = self.files.lock().unwrap().get(remote).cloned();
        match content_opt {
            Some(content) => {
                if let Some(parent) = local.parent() {
                    std::fs::create_dir_all(parent).map_err(|_| TransferError::Io)?;
                }
                std::fs::write(local, content).map_err(|_| TransferError::Io)?;
                Ok(())
            }
            None => Err(TransferError::NotFound),
        }
    }

    fn upload(&self, local: &Path, remote: &str) -> Result<(), TransferError> {
        self.log.lock().unwrap().push(format!("upload:{}", remote));
        if self.fail_upload.as_deref() == Some(remote) {
            return Err(TransferError::Io);
        }
        let content = std::fs::read(local).map_err(|_| TransferError::Io)?;
        self.files.lock().unwrap().insert(remote.to_string(), content);
        Ok(())
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), TransferError> {
        self.log.lock().unwrap().push(format!("rename:{}:{}", src, dst));
        if self.fail_rename_src.as_deref() == Some(src) {
            return Err(TransferError::Io);
        }
        let mut files = self.files.lock().unwrap();
        // 016.12: rename must never target a name that already exists
        assert!(
            !files.contains_key(dst),
            "016.12 violated: rename dst already exists: {:?}",
            dst
        );
        match files.remove(src) {
            Some(content) => {
                files.insert(dst.to_string(), content);
                Ok(())
            }
            None => Err(TransferError::NotFound),
        }
    }

    fn delete(&self, path: &str) -> Result<(), TransferError> {
        self.log.lock().unwrap().push(format!("delete:{}", path));
        if self.files.lock().unwrap().remove(path).is_some() {
            Ok(())
        } else {
            Err(TransferError::NotFound)
        }
    }

    fn delete_dir(&self, path: &str) -> Result<(), TransferError> {
        self.log.lock().unwrap().push(format!("delete_dir:{}", path));
        Ok(())
    }
}

// ---- Helpers ----

fn test_tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir()
        .join("snapshot_transfer_tests")
        .join(name);
    std::fs::remove_dir_all(&p).ok();
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn write_local_db(dir: &Path, content: &[u8]) -> PathBuf {
    let path = dir.join("local.db");
    std::fs::write(&path, content).unwrap();
    path
}

// ---- Download tests ----

// 016.1, 016.4: download fetches from .kitchensync/snapshot.db and places
// the result at {tmp}/{uuid}/snapshot.db
#[test]
fn download_fetches_correct_peer_path_and_places_in_tmp_subdir() {
    let subject = new();
    let peer = FakePeer::with_files(&[LIVE]);
    let tmp = test_tmp("download_path");

    let result = subject.download(&peer, &tmp, false).unwrap();

    assert!(peer.ops().contains(&format!("download:{}", LIVE)));
    assert!(result.local_path.starts_with(&tmp));
    assert_eq!(result.local_path.file_name().unwrap(), "snapshot.db");
    // A uuid directory must sit between tmp and snapshot.db (016.4)
    assert_ne!(result.local_path.parent().unwrap(), tmp.as_path());
    assert!(result.local_path.exists());
}

// 016.5: download does not write to the peer
#[test]
fn download_does_not_modify_peer() {
    let subject = new();
    let peer = FakePeer::with_files(&[LIVE]);
    let tmp = test_tmp("download_no_modify");

    subject.download(&peer, &tmp, false).unwrap();

    assert!(peer.has(LIVE));
    for op in peer.ops() {
        assert!(
            !op.starts_with("upload:")
                && !op.starts_with("rename:")
                && !op.starts_with("delete:"),
            "download must not write to peer: {}",
            op
        );
    }
}

// 016.6: when the peer has no snapshot, create an empty local file and report
// had_history=false
#[test]
fn download_no_peer_snapshot_creates_empty_local_file() {
    let subject = new();
    let peer = FakePeer::empty();
    let tmp = test_tmp("download_no_snapshot");

    let result = subject.download(&peer, &tmp, false).unwrap();

    assert!(!result.had_history);
    assert!(result.local_path.exists());
    assert_eq!(std::fs::read(&result.local_path).unwrap(), b"");
}

// 016.6: when the peer has a snapshot, report had_history=true
#[test]
fn download_existing_snapshot_reports_had_history() {
    let subject = new();
    let peer = FakePeer::with_files(&[LIVE]);
    let tmp = test_tmp("download_had_history");

    let result = subject.download(&peer, &tmp, false).unwrap();

    assert!(result.had_history);
}

// 016.13: had_history reflects the state after recovery, not before
#[test]
fn download_had_history_reflects_post_recovery_state() {
    let subject = new();
    // 016.18: new exists, no old, no live -- recovery renames new to live
    let peer = FakePeer::with_files(&[SWAP_NEW]);
    let tmp = test_tmp("had_history_post_recovery");

    // Without recovery, no live exists and had_history would be false.
    // The caller applies recovery first (016.13).
    subject.recover(&peer, false).unwrap();
    assert!(peer.has(LIVE));

    let result = subject.download(&peer, &tmp, false).unwrap();
    assert!(result.had_history);
}

// ---- Recovery tests ----

// 016.14: old + live -- delete new if present, then delete old
#[test]
fn recover_old_and_live_with_new_deletes_new_then_old() {
    let subject = new();
    let peer = FakePeer::with_files(&[SWAP_OLD, SWAP_NEW, LIVE]);

    subject.recover(&peer, false).unwrap();

    assert!(peer.has(LIVE));
    assert!(!peer.has(SWAP_NEW));
    assert!(!peer.has(SWAP_OLD));
    let ops = peer.ops();
    let del_new = ops.iter().position(|o| o == &format!("delete:{}", SWAP_NEW)).unwrap();
    let del_old = ops.iter().position(|o| o == &format!("delete:{}", SWAP_OLD)).unwrap();
    assert!(del_new < del_old, "016.14: delete new before delete old");
}

// 016.14: old + live, no new -- delete old
#[test]
fn recover_old_and_live_without_new_deletes_old() {
    let subject = new();
    let peer = FakePeer::with_files(&[SWAP_OLD, LIVE]);

    subject.recover(&peer, false).unwrap();

    assert!(peer.has(LIVE));
    assert!(!peer.has(SWAP_OLD));
}

// 016.15: old + new, no live -- rename new to live, then delete old
#[test]
fn recover_old_and_new_no_live_restores_snapshot() {
    let subject = new();
    let peer = FakePeer::with_files(&[SWAP_OLD, SWAP_NEW]);

    subject.recover(&peer, false).unwrap();

    assert!(peer.has(LIVE));
    assert!(!peer.has(SWAP_NEW));
    assert!(!peer.has(SWAP_OLD));
    let ops = peer.ops();
    let rename_pos = ops.iter().position(|o| o.starts_with("rename:")).unwrap();
    let del_old = ops.iter().position(|o| o == &format!("delete:{}", SWAP_OLD)).unwrap();
    assert!(rename_pos < del_old, "016.15: rename new before delete old");
}

// 016.16: old only, no new, no live -- rename old to live
#[test]
fn recover_old_no_new_no_live_renames_old_to_live() {
    let subject = new();
    let peer = FakePeer::with_files(&[SWAP_OLD]);

    subject.recover(&peer, false).unwrap();

    assert!(peer.has(LIVE));
    assert!(!peer.has(SWAP_OLD));
}

// 016.17: new + live, no old -- delete new
#[test]
fn recover_no_old_new_and_live_deletes_new() {
    let subject = new();
    let peer = FakePeer::with_files(&[SWAP_NEW, LIVE]);

    subject.recover(&peer, false).unwrap();

    assert!(!peer.has(SWAP_NEW));
    assert!(peer.has(LIVE));
}

// 016.18: new only, no old, no live -- rename new to live
#[test]
fn recover_no_old_new_no_live_renames_new_to_live() {
    let subject = new();
    let peer = FakePeer::with_files(&[SWAP_NEW]);

    subject.recover(&peer, false).unwrap();

    assert!(peer.has(LIVE));
    assert!(!peer.has(SWAP_NEW));
}

// No SWAP state -- peer left untouched
#[test]
fn recover_no_swap_state_leaves_peer_unchanged() {
    let subject = new();
    let peer = FakePeer::with_files(&[LIVE]);

    subject.recover(&peer, false).unwrap();

    assert!(peer.has(LIVE));
    for op in peer.ops() {
        assert!(
            !op.starts_with("rename:")
                && !op.starts_with("delete:")
                && !op.starts_with("upload:"),
            "clean peer should need no recovery ops: {}",
            op
        );
    }
}

// ---- Upload tests ----

// 016.7: upload sends the caller-provided file content as-is
#[test]
fn upload_sends_local_db_content_to_peer() {
    let subject = new();
    let peer = FakePeer::empty();
    let tmp = test_tmp("upload_content");
    let content = b"sqlite-binary-blob";
    let local_db = write_local_db(&tmp, content);

    subject.upload(&peer, &local_db, false).unwrap();

    assert_eq!(peer.content(LIVE).as_deref(), Some(content.as_slice()));
}

// 016.8, 016.9, 016.10, 016.11: upload sequence when live exists
// -- write SWAP/new, rename live to old, rename new to live, delete old
#[test]
fn upload_sequence_with_existing_live() {
    let subject = new();
    let peer = FakePeer::with_files(&[LIVE]);
    let tmp = test_tmp("upload_seq_live");
    let local_db = write_local_db(&tmp, b"new-snapshot");

    subject.upload(&peer, &local_db, false).unwrap();

    let ops = peer.ops();
    let up_new = format!("upload:{}", SWAP_NEW);
    let ren_to_old = format!("rename:{}:{}", LIVE, SWAP_OLD);
    let ren_to_live = format!("rename:{}:{}", SWAP_NEW, LIVE);
    let del_old = format!("delete:{}", SWAP_OLD);

    let p0 = ops.iter().position(|o| o == &up_new).expect("upload SWAP/new missing");
    let p1 = ops.iter().position(|o| o == &ren_to_old).expect("rename live->old missing");
    let p2 = ops.iter().position(|o| o == &ren_to_live).expect("rename new->live missing");
    let p3 = ops.iter().position(|o| o == &del_old).expect("delete old missing");

    assert!(p0 < p1, "016.8: write SWAP/new before rename live to old");
    assert!(p1 < p2, "016.9: rename live to old before rename new to live");
    assert!(p2 < p3, "016.10/11: rename new to live before delete old");

    assert!(peer.has(LIVE));
    assert!(!peer.has(SWAP_NEW));
    assert!(!peer.has(SWAP_OLD));
}

// 016.8, 016.10: upload sequence when live is absent -- write SWAP/new, rename new to live
#[test]
fn upload_sequence_without_existing_live() {
    let subject = new();
    let peer = FakePeer::empty();
    let tmp = test_tmp("upload_seq_no_live");
    let local_db = write_local_db(&tmp, b"first-snapshot");

    subject.upload(&peer, &local_db, false).unwrap();

    let ops = peer.ops();
    assert!(ops.iter().any(|o| o == &format!("upload:{}", SWAP_NEW)));
    assert!(ops.iter().any(|o| o == &format!("rename:{}:{}", SWAP_NEW, LIVE)));
    // No old involved when live was absent
    assert!(!ops.iter().any(|o| o.contains(SWAP_OLD)));

    assert!(peer.has(LIVE));
    assert!(!peer.has(SWAP_NEW));
}

// 016.3: upload never references SQLite sidecar paths
#[test]
fn upload_never_touches_sidecar_files() {
    let subject = new();
    let peer = FakePeer::empty();
    let tmp = test_tmp("upload_no_sidecars");
    let local_db = write_local_db(&tmp, b"snapshot");

    subject.upload(&peer, &local_db, false).unwrap();

    for op in peer.ops() {
        for suf in &["-journal", "-wal", "-shm"] {
            assert!(!op.contains(suf), "sidecar referenced in op: {}", op);
        }
    }
}

// 016.12: rename never targets an existing name (FakePeer.rename asserts this;
// a successful upload with an existing live proves the invariant holds)
#[test]
fn upload_rename_never_targets_existing_name() {
    let subject = new();
    let peer = FakePeer::with_files(&[LIVE]);
    let tmp = test_tmp("rename_no_overwrite");
    let local_db = write_local_db(&tmp, b"data");

    subject.upload(&peer, &local_db, false).unwrap();
}

// 016.19: the run that uploads last wins
#[test]
fn upload_last_writer_wins() {
    let subject = new();
    let peer = FakePeer::empty();
    let tmp = test_tmp("last_writer_wins");
    let db1 = write_local_db(&tmp, b"run-1");
    let db2 = tmp.join("run2.db");
    std::fs::write(&db2, b"run-2").unwrap();

    subject.upload(&peer, &db1, false).unwrap();
    subject.upload(&peer, &db2, false).unwrap();

    assert_eq!(peer.content(LIVE).as_deref(), Some(b"run-2".as_slice()));
}

// 016.20: upload fails before old exists -- live kept, SWAP/new left for recovery
#[test]
fn upload_fail_before_old_leaves_live_intact() {
    let subject = new();
    let peer = FakePeer::with_files(&[LIVE]).failing_upload(SWAP_NEW);
    let tmp = test_tmp("upload_fail_before_old");
    let local_db = write_local_db(&tmp, b"new-data");

    let result = subject.upload(&peer, &local_db, false);
    assert!(result.is_err());

    assert!(peer.has(LIVE));
    assert!(!peer.has(SWAP_OLD));
}

// 016.21: upload fails after old exists -- SWAP state left in place for next
// run's startup recovery
#[test]
fn upload_fail_after_old_leaves_swap_state_for_recovery() {
    let subject = new();
    // Fail when renaming SWAP_NEW -> LIVE (after LIVE has been renamed to SWAP_OLD)
    let peer = FakePeer::with_files(&[LIVE]).failing_rename_of(SWAP_NEW);
    let tmp = test_tmp("upload_fail_after_old");
    let local_db = write_local_db(&tmp, b"new-data");

    let result = subject.upload(&peer, &local_db, false);
    assert!(result.is_err());

    assert!(peer.has(SWAP_OLD), "old must remain for recovery");
    assert!(peer.has(SWAP_NEW), "new must remain for recovery");
    assert!(!peer.has(LIVE));
}

// ---- Dry-run tests ----

// 024.2: recover with dry_run skips all peer-side operations
#[test]
fn recover_dry_run_skips_peer_operations() {
    let subject = new();
    // State that would trigger 016.18 recovery under a normal run
    let peer = FakePeer::with_files(&[SWAP_NEW]);

    subject.recover(&peer, true).unwrap();

    assert!(peer.has(SWAP_NEW));
    assert!(!peer.has(LIVE));
    for op in peer.ops() {
        assert!(
            !op.starts_with("rename:")
                && !op.starts_with("delete:")
                && !op.starts_with("upload:"),
            "dry_run recover must not mutate peer: {}",
            op
        );
    }
}

// 024.3: download with dry_run still fetches the live snapshot
#[test]
fn download_dry_run_still_fetches_live_snapshot() {
    let subject = new();
    let peer = FakePeer::with_files(&[LIVE]);
    let tmp = test_tmp("download_dry_run");

    let result = subject.download(&peer, &tmp, true).unwrap();

    assert!(result.local_path.exists());
    assert!(result.had_history);
    assert!(peer.ops().contains(&format!("download:{}", LIVE)));
}

// 024.3: download with dry_run creates local empty file when peer has no snapshot
#[test]
fn download_dry_run_no_peer_snapshot_creates_local_file() {
    let subject = new();
    let peer = FakePeer::empty();
    let tmp = test_tmp("download_dry_run_no_snap");

    let result = subject.download(&peer, &tmp, true).unwrap();

    assert!(result.local_path.exists());
    assert!(!result.had_history);
}

// 024.18: upload with dry_run skips all peer-side operations
#[test]
fn upload_dry_run_skips_peer_operations() {
    let subject = new();
    let peer = FakePeer::with_files(&[LIVE]);
    let tmp = test_tmp("upload_dry_run");
    let local_db = write_local_db(&tmp, b"new-snapshot");

    subject.upload(&peer, &local_db, true).unwrap();

    assert!(peer.has(LIVE));
    assert!(!peer.has(SWAP_NEW));
    assert!(!peer.has(SWAP_OLD));
    assert!(peer.ops().is_empty());
}
