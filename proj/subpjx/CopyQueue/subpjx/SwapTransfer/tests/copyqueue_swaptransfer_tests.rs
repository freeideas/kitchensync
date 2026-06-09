use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use copyqueue_swaptransfer::{
    new, Fs, FsError, ReadHandle, SwapTransfer, TransferOutcome, WriteHandle,
};

// ---------------------------------------------------------------------------
// In-memory Fs for testing
// ---------------------------------------------------------------------------

struct MemInner {
    files: HashMap<String, Vec<u8>>,
    mod_times: HashMap<String, SystemTime>,
    open_write_paths: Vec<String>,
    dirs_created: Vec<String>,
    mod_time_sets: Vec<(String, SystemTime)>,
    read_chunk_sizes: Vec<usize>,
    next_id: u64,
    rh: HashMap<u64, (String, usize)>,   // id -> (path, offset)
    wh: HashMap<u64, (String, Vec<u8>)>, // id -> (path, buffer)
    // Failure injection
    fail_rename_to: Option<String>,
    fail_rename_from: Option<String>,
    fail_set_mod_time: bool,
    fail_exists: bool,
    native_copy_enabled: bool,
}

impl MemInner {
    fn new() -> Self {
        MemInner {
            files: HashMap::new(),
            mod_times: HashMap::new(),
            open_write_paths: Vec::new(),
            dirs_created: Vec::new(),
            mod_time_sets: Vec::new(),
            read_chunk_sizes: Vec::new(),
            next_id: 1,
            rh: HashMap::new(),
            wh: HashMap::new(),
            fail_rename_to: None,
            fail_rename_from: None,
            fail_set_mod_time: false,
            fail_exists: false,
            native_copy_enabled: false,
        }
    }
}

// Shared event tape for streaming-order tests: 0 = read-chunk, 1 = write-chunk.
type Tape = Arc<Mutex<Vec<u8>>>;

struct MemFs {
    inner: Mutex<MemInner>,
    tape: Option<Tape>,
}

impl MemFs {
    fn new() -> Self {
        MemFs { inner: Mutex::new(MemInner::new()), tape: None }
    }

    fn with_tape(tape: Tape) -> Self {
        MemFs { inner: Mutex::new(MemInner::new()), tape: Some(tape) }
    }

    fn put(&self, path: &str, data: &[u8]) {
        self.inner.lock().unwrap().files.insert(path.to_string(), data.to_vec());
    }

    fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.inner.lock().unwrap().files.get(path).cloned()
    }

    fn has(&self, path: &str) -> bool {
        self.inner.lock().unwrap().files.contains_key(path)
    }

    fn has_any_file(&self) -> bool {
        !self.inner.lock().unwrap().files.is_empty()
    }

    fn dirs_created(&self) -> Vec<String> {
        self.inner.lock().unwrap().dirs_created.clone()
    }

    fn mod_time_sets(&self) -> Vec<(String, SystemTime)> {
        self.inner.lock().unwrap().mod_time_sets.clone()
    }

    fn read_chunk_sizes(&self) -> Vec<usize> {
        self.inner.lock().unwrap().read_chunk_sizes.clone()
    }

    fn open_write_paths(&self) -> Vec<String> {
        self.inner.lock().unwrap().open_write_paths.clone()
    }

    fn set_fail_rename_to(&self, dst: &str) {
        self.inner.lock().unwrap().fail_rename_to = Some(dst.to_string());
    }

    fn set_fail_rename_from(&self, src: &str) {
        self.inner.lock().unwrap().fail_rename_from = Some(src.to_string());
    }

    fn set_fail_exists(&self) {
        self.inner.lock().unwrap().fail_exists = true;
    }

    fn enable_native_copy(&self) {
        self.inner.lock().unwrap().native_copy_enabled = true;
    }
}

impl Fs for MemFs {
    fn open_read(&self, path: &str) -> Result<ReadHandle, FsError> {
        let mut g = self.inner.lock().unwrap();
        if !g.files.contains_key(path) {
            return Err(FsError);
        }
        let id = g.next_id;
        g.next_id += 1;
        g.rh.insert(id, (path.to_string(), 0));
        Ok(ReadHandle(id))
    }

    fn read(&self, handle: &ReadHandle, max_bytes: usize) -> Result<Option<Vec<u8>>, FsError> {
        let mut g = self.inner.lock().unwrap();
        g.read_chunk_sizes.push(max_bytes);
        let (path, off) = g.rh.get(&handle.0).cloned().ok_or(FsError)?;
        let content = g.files.get(&path).cloned().ok_or(FsError)?;
        if off >= content.len() {
            return Ok(None);
        }
        let end = (off + max_bytes).min(content.len());
        let chunk = content[off..end].to_vec();
        g.rh.insert(handle.0, (path, end));
        drop(g); // release before touching tape
        if let Some(tape) = &self.tape {
            tape.lock().unwrap().push(0);
        }
        Ok(Some(chunk))
    }

    fn close_read(&self, handle: ReadHandle) -> Result<(), FsError> {
        self.inner.lock().unwrap().rh.remove(&handle.0);
        Ok(())
    }

    fn open_write(&self, path: &str) -> Result<WriteHandle, FsError> {
        let mut g = self.inner.lock().unwrap();
        g.open_write_paths.push(path.to_string());
        let id = g.next_id;
        g.next_id += 1;
        g.wh.insert(id, (path.to_string(), Vec::new()));
        Ok(WriteHandle(id))
    }

    fn write(&self, handle: &WriteHandle, bytes: &[u8]) -> Result<(), FsError> {
        let mut g = self.inner.lock().unwrap();
        let (_, buf) = g.wh.get_mut(&handle.0).ok_or(FsError)?;
        buf.extend_from_slice(bytes);
        drop(g);
        if let Some(tape) = &self.tape {
            tape.lock().unwrap().push(1);
        }
        Ok(())
    }

    fn close_write(&self, handle: WriteHandle) -> Result<(), FsError> {
        let mut g = self.inner.lock().unwrap();
        let (path, buf) = g.wh.remove(&handle.0).ok_or(FsError)?;
        g.files.insert(path, buf);
        Ok(())
    }

    fn create_dir(&self, path: &str) -> Result<(), FsError> {
        self.inner.lock().unwrap().dirs_created.push(path.to_string());
        Ok(())
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), FsError> {
        let mut g = self.inner.lock().unwrap();
        if g.fail_rename_to.as_deref() == Some(dst) {
            return Err(FsError);
        }
        if g.fail_rename_from.as_deref() == Some(src) {
            return Err(FsError);
        }
        let data = g.files.remove(src).ok_or(FsError)?;
        g.files.insert(dst.to_string(), data);
        Ok(())
    }

    fn delete_file(&self, path: &str) -> Result<(), FsError> {
        self.inner.lock().unwrap().files.remove(path);
        Ok(())
    }

    fn delete_dir(&self, _path: &str) -> Result<(), FsError> {
        Ok(())
    }

    fn exists(&self, path: &str) -> Result<bool, FsError> {
        let g = self.inner.lock().unwrap();
        if g.fail_exists {
            return Err(FsError);
        }
        Ok(g.files.contains_key(path))
    }

    fn set_mod_time(&self, path: &str, time: SystemTime) -> Result<(), FsError> {
        let mut g = self.inner.lock().unwrap();
        if g.fail_set_mod_time {
            return Err(FsError);
        }
        g.mod_times.insert(path.to_string(), time);
        g.mod_time_sets.push((path.to_string(), time));
        Ok(())
    }

    fn native_copy(&self, src: &dyn Fs, src_path: &str, dst_path: &str) -> Result<(), FsError> {
        if !self.inner.lock().unwrap().native_copy_enabled {
            return Err(FsError);
        }
        let rh = src.open_read(src_path)?;
        let wh = self.open_write(dst_path)?;
        loop {
            match src.read(&rh, 65536)? {
                Some(chunk) => self.write(&wh, &chunk)?,
                None => break,
            }
        }
        src.close_read(rh)?;
        self.close_write(wh)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ts(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

// Canonical paths used across transfer tests.
const SRC_PATH: &str = "src.txt";
const DST_PATH: &str = "dst/file.txt";
const TMP_DIR: &str = "dst/.kitchensync/TMP/1";
const BAK_DEST: &str = "dst/.kitchensync/BAK/ts/file.txt";
const SWAP_NEW: &str = "dst/.kitchensync/SWAP/file.txt/new";
const SWAP_OLD: &str = "dst/.kitchensync/SWAP/file.txt/old";

// ---------------------------------------------------------------------------
// transfer: happy path
// ---------------------------------------------------------------------------

// 019.1, 019.3: source content staged to SWAP new, then renamed to the target path.
#[test]
fn transfer_writes_new_content_to_target() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"hello world");

    let out = svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1000), TMP_DIR, BAK_DEST, false);

    assert!(matches!(out, TransferOutcome::Done));
    assert_eq!(dst.get(DST_PATH).as_deref(), Some(b"hello world" as &[u8]));
}

// 019.4: the modification time set on the destination is the winning mod_time
// supplied with the request, not a value re-read from the source.
#[test]
fn transfer_sets_winning_mod_time_on_target() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"data");
    let winning = ts(9999);

    svc.transfer(&src, SRC_PATH, &dst, DST_PATH, winning, TMP_DIR, BAK_DEST, false);

    let sets = dst.mod_time_sets();
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].0, DST_PATH);
    assert_eq!(sets[0].1, winning);
}

// 019.6: SWAP staging files are removed after a successful replacement.
#[test]
fn transfer_removes_swap_files_after_success() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"data");

    svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, false);

    assert!(!dst.has(SWAP_NEW));
    assert!(!dst.has(SWAP_OLD));
}

// 019.2, 019.5: when a file already exists at the target, it is moved to SWAP old
// before the new content is swapped in, and afterwards archived to BAK.
#[test]
fn transfer_archives_displaced_existing_target_to_bak() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"new content");
    dst.put(DST_PATH, b"old content");

    let out = svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(2), TMP_DIR, BAK_DEST, false);

    assert!(matches!(out, TransferOutcome::Done));
    assert_eq!(dst.get(DST_PATH).as_deref(), Some(b"new content" as &[u8]));
    assert_eq!(dst.get(BAK_DEST).as_deref(), Some(b"old content" as &[u8]));
}

// ---------------------------------------------------------------------------
// transfer: error obligations
// ---------------------------------------------------------------------------

// 019.9: when failure occurs before SWAP old is created, the staged SWAP new
// file is deleted and Failed is reported.
#[test]
fn transfer_cleans_staged_new_on_pre_old_failure() {
    // No existing target (old is never created). Fail the rename of new to target.
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"data");
    dst.set_fail_rename_to(DST_PATH);

    let out = svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, false);

    assert!(matches!(out, TransferOutcome::Failed));
    assert!(!dst.has(SWAP_NEW));
    assert!(!dst.has(DST_PATH));
}

// 019.10: when moving the existing target to SWAP old fails, the original
// destination file remains in place.
#[test]
fn transfer_leaves_original_target_when_move_to_old_fails() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"new data");
    dst.put(DST_PATH, b"original");
    dst.set_fail_rename_from(DST_PATH);

    svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, false);

    assert_eq!(dst.get(DST_PATH).as_deref(), Some(b"original" as &[u8]));
}

// 019.11: when moving the existing target to SWAP old fails, Skipped is reported
// so the scheduler does not requeue the copy.
#[test]
fn transfer_skips_when_move_to_old_fails() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"new data");
    dst.put(DST_PATH, b"original");
    dst.set_fail_rename_from(DST_PATH);

    let out = svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, false);

    assert!(matches!(out, TransferOutcome::Skipped));
}

// 019.12: when failure occurs after SWAP old exists, the SWAP state is left
// in place for a later recovery pass and Failed is reported.
#[test]
fn transfer_leaves_swap_state_on_failure_after_old_created() {
    // Existing target: step 3 succeeds (old created). Fail step 4 (rename new to target).
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"new data");
    dst.put(DST_PATH, b"original");
    dst.set_fail_rename_to(DST_PATH);

    let out = svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, false);

    assert!(matches!(out, TransferOutcome::Failed));
    assert!(dst.has(SWAP_NEW));
    assert!(dst.has(SWAP_OLD));
}

// 019.13: when archiving SWAP old to BAK fails after the replacement is in place,
// SWAP old is left for later recovery and Done is still reported.
#[test]
fn transfer_reports_done_and_leaves_old_when_bak_archive_fails() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"new data");
    dst.put(DST_PATH, b"original");
    dst.set_fail_rename_to(BAK_DEST); // archival to BAK fails

    let out = svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, false);

    assert!(matches!(out, TransferOutcome::Done));
    assert_eq!(dst.get(DST_PATH).as_deref(), Some(b"new data" as &[u8]));
    assert!(dst.has(SWAP_OLD)); // left for later recovery
}

// ---------------------------------------------------------------------------
// transfer: SWAP path encoding and pre-transfer recovery
// ---------------------------------------------------------------------------

// 019.7: the SWAP directory segment is the target basename percent-encoded.
// Verified through recover: a SWAP entry at the encoded path is found and
// reconciled when recover is given the raw (decoded) target path.
#[test]
fn recover_uses_percent_encoded_basename_for_swap_path() {
    // "hello world.txt" encodes to "hello%20world.txt".
    // State 019.18 (no old, new present, target present): recovery deletes new.
    let svc = new();
    let fs = MemFs::new();
    fs.put("dst/hello world.txt", b"live");
    fs.put("dst/.kitchensync/SWAP/hello%20world.txt/new", b"stale");

    let ok = svc.recover(
        &fs,
        "dst/hello world.txt",
        "dst/.kitchensync/BAK/ts/hello%20world.txt",
        false,
    );

    assert!(ok);
    // new was located at the encoded path and deleted.
    assert!(!fs.has("dst/.kitchensync/SWAP/hello%20world.txt/new"));
    // target is untouched.
    assert_eq!(fs.get("dst/hello world.txt").as_deref(), Some(b"live" as &[u8]));
}

// 019.8: a pre-existing SWAP directory for the target basename is recovered
// before the new transfer is staged.
#[test]
fn transfer_recovers_preexisting_swap_before_staging() {
    // Pre-existing state 019.18: stale SWAP new with target present.
    // Recovery deletes the stale new. Transfer then stages and commits fresh content.
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"fresh content");
    dst.put(DST_PATH, b"current content");
    dst.put(SWAP_NEW, b"stale interrupted content");

    let out = svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(3), TMP_DIR, BAK_DEST, false);

    assert!(matches!(out, TransferOutcome::Done));
    assert_eq!(dst.get(DST_PATH).as_deref(), Some(b"fresh content" as &[u8]));
    // The pre-existing SWAP new is gone (deleted by recovery, not left or archived).
    assert!(!dst.has(SWAP_NEW));
    // The original current content was displaced to BAK during the transfer itself.
    assert_eq!(dst.get(BAK_DEST).as_deref(), Some(b"current content" as &[u8]));
}

// ---------------------------------------------------------------------------
// transfer: streaming and buffering
// ---------------------------------------------------------------------------

// 020.13: the max_bytes argument to Fs::read is a fixed constant independent
// of the size of the file being copied.
#[test]
fn transfer_uses_fixed_chunk_size_for_reads() {
    const CHUNK: usize = 65536;
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    // File larger than one chunk.
    src.put(SRC_PATH, &vec![0u8; CHUNK + 1]);

    svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, false);

    let sizes = src.read_chunk_sizes();
    assert!(!sizes.is_empty());
    for &s in &sizes {
        assert_eq!(s, CHUNK, "read called with non-constant max_bytes {s}");
    }
}

// 020.14: writing to the destination begins before the entire source file has
// been read into memory.
#[test]
fn transfer_begins_writing_before_all_source_bytes_read() {
    const CHUNK: usize = 65536;
    // Shared tape records the interleaved sequence: 0 = src read-chunk, 1 = dst write-chunk.
    let tape: Tape = Arc::new(Mutex::new(Vec::new()));
    let src = MemFs::with_tape(Arc::clone(&tape));
    let dst = MemFs::with_tape(Arc::clone(&tape));
    // Two-chunk file forces at least two read+write rounds.
    src.put(SRC_PATH, &vec![1u8; CHUNK * 2]);

    let svc = new();
    svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, false);

    let events = tape.lock().unwrap().clone();
    let first_write_pos = events.iter().position(|&e| e == 1).expect("no write events");
    let last_read_pos = events.iter().rposition(|&e| e == 0).expect("no read events");
    // The first write chunk arrived before the last read chunk, proving that
    // bytes were delivered to the destination before the source was fully read.
    assert!(
        first_write_pos < last_read_pos,
        "first write at {first_write_pos} must precede last read at {last_read_pos}",
    );
}

// 020.15: when the native local copy path is taken, the copy still goes through
// the SWAP new path rather than writing the target in place.
#[test]
fn transfer_via_native_copy_stages_through_swap_new() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"native content");
    dst.enable_native_copy();

    let out = svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, false);

    assert!(matches!(out, TransferOutcome::Done));
    assert_eq!(dst.get(DST_PATH).as_deref(), Some(b"native content" as &[u8]));
    // The final target path was never opened directly for writing; the copy
    // passed through SWAP new and was renamed into place.
    let wrote_to = dst.open_write_paths();
    assert!(!wrote_to.contains(&DST_PATH.to_string()));
    assert!(wrote_to.iter().any(|p| p.ends_with("/new")));
}

// ---------------------------------------------------------------------------
// transfer: dry-run
// ---------------------------------------------------------------------------

// 024.5: a dry-run transfer still reads the source file.
#[test]
fn transfer_dry_run_reads_source_file() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"source data");

    svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, true);

    assert!(!src.read_chunk_sizes().is_empty());
}

// 024.13: a dry-run transfer creates no TMP, SWAP, or BAK directories on the peer.
#[test]
fn transfer_dry_run_creates_no_dirs() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"data");

    svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, true);

    assert!(dst.dirs_created().is_empty());
}

// 024.14: a dry-run transfer writes no destination files on the peer.
#[test]
fn transfer_dry_run_writes_no_destination_files() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"data");

    svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, true);

    assert!(!dst.has_any_file());
}

// 024.17: a dry-run transfer sets no modification times on the peer.
#[test]
fn transfer_dry_run_sets_no_mod_time() {
    let svc = new();
    let src = MemFs::new();
    let dst = MemFs::new();
    src.put(SRC_PATH, b"data");

    svc.transfer(&src, SRC_PATH, &dst, DST_PATH, ts(1), TMP_DIR, BAK_DEST, true);

    assert!(dst.mod_time_sets().is_empty());
}

// ---------------------------------------------------------------------------
// recover: the five states
// ---------------------------------------------------------------------------

// 019.15: old present + target present -> move old to BAK, remove empty SWAP dir.
#[test]
fn recover_old_and_target_archives_old_to_bak() {
    let svc = new();
    let fs = MemFs::new();
    fs.put(DST_PATH, b"live");
    fs.put(SWAP_OLD, b"superseded");

    let ok = svc.recover(&fs, DST_PATH, BAK_DEST, false);

    assert!(ok);
    assert!(!fs.has(SWAP_OLD));
    assert_eq!(fs.get(BAK_DEST).as_deref(), Some(b"superseded" as &[u8]));
    assert_eq!(fs.get(DST_PATH).as_deref(), Some(b"live" as &[u8]));
}

// 019.16: old present + new present + target missing -> rename new to target,
// move old to BAK, remove empty SWAP dir.
#[test]
fn recover_old_and_new_no_target_completes_replacement() {
    let svc = new();
    let fs = MemFs::new();
    fs.put(SWAP_OLD, b"previous");
    fs.put(SWAP_NEW, b"incoming");

    let ok = svc.recover(&fs, DST_PATH, BAK_DEST, false);

    assert!(ok);
    assert_eq!(fs.get(DST_PATH).as_deref(), Some(b"incoming" as &[u8]));
    assert_eq!(fs.get(BAK_DEST).as_deref(), Some(b"previous" as &[u8]));
    assert!(!fs.has(SWAP_NEW));
    assert!(!fs.has(SWAP_OLD));
}

// 019.17: old present + new missing + target missing -> rename old back to target,
// remove empty SWAP dir.
#[test]
fn recover_old_no_new_no_target_restores_old_to_target() {
    let svc = new();
    let fs = MemFs::new();
    fs.put(SWAP_OLD, b"rolled-back");

    let ok = svc.recover(&fs, DST_PATH, BAK_DEST, false);

    assert!(ok);
    assert_eq!(fs.get(DST_PATH).as_deref(), Some(b"rolled-back" as &[u8]));
    assert!(!fs.has(SWAP_OLD));
}

// 019.18: old missing + new present + target present -> delete new,
// remove empty SWAP dir.
#[test]
fn recover_no_old_new_and_target_deletes_new() {
    let svc = new();
    let fs = MemFs::new();
    fs.put(DST_PATH, b"live");
    fs.put(SWAP_NEW, b"partial-write");

    let ok = svc.recover(&fs, DST_PATH, BAK_DEST, false);

    assert!(ok);
    assert!(!fs.has(SWAP_NEW));
    assert_eq!(fs.get(DST_PATH).as_deref(), Some(b"live" as &[u8]));
}

// 019.19: old missing + new present + target missing -> rename new to target,
// remove empty SWAP dir.
#[test]
fn recover_no_old_new_no_target_promotes_new_to_target() {
    let svc = new();
    let fs = MemFs::new();
    fs.put(SWAP_NEW, b"incoming");

    let ok = svc.recover(&fs, DST_PATH, BAK_DEST, false);

    assert!(ok);
    assert_eq!(fs.get(DST_PATH).as_deref(), Some(b"incoming" as &[u8]));
    assert!(!fs.has(SWAP_NEW));
}

// ---------------------------------------------------------------------------
// recover: error and dry-run
// ---------------------------------------------------------------------------

// 019.20: when the Fs port signals an error during recovery, recover returns
// false so the caller can exclude the peer from that directory subtree.
#[test]
fn recover_returns_false_on_fs_failure() {
    let svc = new();
    let fs = MemFs::new();
    fs.put(SWAP_NEW, b"x"); // something to recover
    fs.set_fail_exists(); // make exists() fail

    let ok = svc.recover(&fs, DST_PATH, BAK_DEST, false);

    assert!(!ok);
}

// 019.21, 024.20: in dry-run, recover skips all peer-side mutations and
// returns true.
#[test]
fn recover_dry_run_skips_mutations() {
    let svc = new();
    let fs = MemFs::new();
    // State 019.19 (no old, new, no target): would rename new to target in a live run.
    fs.put(SWAP_NEW, b"content");

    let ok = svc.recover(&fs, DST_PATH, BAK_DEST, true);

    assert!(ok);
    assert!(fs.has(SWAP_NEW)); // nothing renamed
    assert!(!fs.has(DST_PATH));
}
