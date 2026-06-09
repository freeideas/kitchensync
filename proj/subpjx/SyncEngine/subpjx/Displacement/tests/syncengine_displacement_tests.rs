use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use syncengine_displacement::{new, Displacement, DisplaceOutcome};
use transport;
use output;

// ---------------------------------------------------------------------------
// Test Transport
// ---------------------------------------------------------------------------

struct TxInner {
    dirs_created: Vec<String>,
    renames: Vec<(String, String)>,
    fail_rename: bool,
}

struct TestTransport {
    inner: Mutex<TxInner>,
}

impl TestTransport {
    fn new() -> Arc<Self> {
        Arc::new(TestTransport {
            inner: Mutex::new(TxInner {
                dirs_created: Vec::new(),
                renames: Vec::new(),
                fail_rename: false,
            }),
        })
    }

    fn dirs_created(&self) -> Vec<String> {
        self.inner.lock().unwrap().dirs_created.clone()
    }

    fn renames(&self) -> Vec<(String, String)> {
        self.inner.lock().unwrap().renames.clone()
    }

    fn set_fail_rename(&self) {
        self.inner.lock().unwrap().fail_rename = true;
    }
}

impl transport::Transport for TestTransport {
    fn normalize_url(&self, _url: &str) -> String { unimplemented!() }
    fn open_peer(&self, _p: &str, _f: &[String], _d: bool, _t: Duration) -> Option<transport::ConnectedPeer> { unimplemented!() }
    fn list_dir(&self, _peer: &transport::PeerHandle, _path: &str) -> Result<Vec<transport::DirEntry>, transport::TransportError> { unimplemented!() }
    fn stat(&self, _peer: &transport::PeerHandle, _path: &str) -> Result<transport::Stat, transport::TransportError> { unimplemented!() }
    fn open_read(&self, _peer: &transport::PeerHandle, _path: &str) -> Result<transport::ReadHandle, transport::TransportError> { unimplemented!() }
    fn read(&self, _h: &transport::ReadHandle, _n: usize) -> Result<Option<Vec<u8>>, transport::TransportError> { unimplemented!() }
    fn close_read(&self, _h: transport::ReadHandle) -> Result<(), transport::TransportError> { unimplemented!() }
    fn open_write(&self, _peer: &transport::PeerHandle, _path: &str) -> Result<transport::WriteHandle, transport::TransportError> { unimplemented!() }
    fn write(&self, _h: &transport::WriteHandle, _b: &[u8]) -> Result<(), transport::TransportError> { unimplemented!() }
    fn close_write(&self, _h: transport::WriteHandle) -> Result<(), transport::TransportError> { unimplemented!() }
    fn delete_file(&self, _peer: &transport::PeerHandle, _path: &str) -> Result<(), transport::TransportError> { unimplemented!() }
    fn delete_dir(&self, _peer: &transport::PeerHandle, _path: &str) -> Result<(), transport::TransportError> { unimplemented!() }
    fn set_mod_time(&self, _peer: &transport::PeerHandle, _path: &str, _t: SystemTime) -> Result<(), transport::TransportError> { unimplemented!() }

    fn create_dir(&self, _peer: &transport::PeerHandle, path: &str) -> Result<(), transport::TransportError> {
        self.inner.lock().unwrap().dirs_created.push(path.to_string());
        Ok(())
    }

    fn rename(&self, _peer: &transport::PeerHandle, src: &str, dst: &str) -> Result<(), transport::TransportError> {
        let mut g = self.inner.lock().unwrap();
        if g.fail_rename {
            return Err(transport::TransportError::Io);
        }
        g.renames.push((src.to_string(), dst.to_string()));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Test Output
// ---------------------------------------------------------------------------

struct TestOutput {
    diagnostics: Mutex<Vec<String>>,
}

impl TestOutput {
    fn new() -> Arc<Self> {
        Arc::new(TestOutput { diagnostics: Mutex::new(Vec::new()) })
    }

    fn diagnostics(&self) -> Vec<String> {
        self.diagnostics.lock().unwrap().clone()
    }
}

impl output::Output for TestOutput {
    fn set_verbosity(&self, _level: output::Verbosity) {}
    fn copied(&self, _relpath: &str) {}
    fn displaced(&self, _relpath: &str) {}
    fn transfer_failed(&self, _r: &str, _u: &str, _p: output::FailedPhase, _e: Option<&str>) {}
    fn copy_slots(&self, _active: usize, _max: usize) {}
    fn diagnostic(&self, message: &str) {
        self.diagnostics.lock().unwrap().push(message.to_string());
    }
}

// ---------------------------------------------------------------------------
// 024.15, 024.16: dry-run suppresses all transport calls and returns Displaced
// ---------------------------------------------------------------------------

#[test]
fn dry_run_returns_displaced_and_calls_no_transport() {
    let subject = new();
    let tx = TestTransport::new();
    let out = TestOutput::new();
    let peer = transport::PeerHandle(0);

    let outcome = subject.displace(&*tx, &*out, &peer, "sync/sub", "item.txt", "ts-20260101", true);

    assert!(
        matches!(outcome, DisplaceOutcome::Displaced),
        "dry-run must return Displaced so the facade can report what would have happened"
    );
    assert!(
        tx.dirs_created().is_empty(),
        "dry-run must not create any directories; got: {:?}",
        tx.dirs_created()
    );
    assert!(
        tx.renames().is_empty(),
        "dry-run must not rename any entries; got: {:?}",
        tx.renames()
    );
}

// ---------------------------------------------------------------------------
// 021.1: BAK timestamp directory created at <parent>/.kitchensync/BAK/<timestamp>/
// ---------------------------------------------------------------------------

#[test]
fn displace_creates_bak_timestamp_directory() {
    let subject = new();
    let tx = TestTransport::new();
    let out = TestOutput::new();
    let peer = transport::PeerHandle(0);

    subject.displace(&*tx, &*out, &peer, "the/parent", "file.txt", "ts-2026", false);

    let dirs = tx.dirs_created();
    assert!(
        dirs.iter().any(|d| d == "the/parent/.kitchensync/BAK/ts-2026"),
        "BAK timestamp directory must be created before the rename; dirs: {:?}",
        dirs
    );
}

// ---------------------------------------------------------------------------
// 021.2: Entry renamed from <parent>/<basename> to <parent>/.kitchensync/BAK/<timestamp>/<basename>
// ---------------------------------------------------------------------------

#[test]
fn displace_renames_entry_preserving_basename() {
    let subject = new();
    let tx = TestTransport::new();
    let out = TestOutput::new();
    let peer = transport::PeerHandle(0);

    subject.displace(&*tx, &*out, &peer, "the/parent", "entry.txt", "ts-2026", false);

    let renames = tx.renames();
    assert_eq!(renames.len(), 1, "exactly one rename must occur");
    assert_eq!(
        renames[0].0, "the/parent/entry.txt",
        "rename source must be the original path"
    );
    assert_eq!(
        renames[0].1, "the/parent/.kitchensync/BAK/ts-2026/entry.txt",
        "rename destination must be under BAK with the original basename"
    );
}

// ---------------------------------------------------------------------------
// 021.3: A directory is moved as a single rename (not per-entry copy+delete)
// ---------------------------------------------------------------------------

#[test]
fn displace_directory_is_a_single_rename() {
    let subject = new();
    let tx = TestTransport::new();
    let out = TestOutput::new();
    let peer = transport::PeerHandle(0);

    // "dir_entry" could be a directory; displacement must still be exactly one rename
    subject.displace(&*tx, &*out, &peer, "root", "dir_entry", "ts-2026", false);

    assert_eq!(
        tx.renames().len(),
        1,
        "a directory displacement must be exactly one rename so the entire subtree travels with it"
    );
}

// ---------------------------------------------------------------------------
// 021.4: BAK directory co-located at displaced entry's parent, not sync root
// ---------------------------------------------------------------------------

#[test]
fn displace_bak_is_at_parent_not_at_sync_root() {
    let subject = new();
    let tx = TestTransport::new();
    let out = TestOutput::new();
    let peer = transport::PeerHandle(0);

    // The sync root is "syncroot"; the entry lives inside "syncroot/sub"; BAK must be
    // at "syncroot/sub/.kitchensync/..." not at "syncroot/.kitchensync/..."
    subject.displace(&*tx, &*out, &peer, "syncroot/sub", "orphan.txt", "ts-2026", false);

    let dirs = tx.dirs_created();
    assert!(
        dirs.iter().any(|d| d.starts_with("syncroot/sub/.kitchensync/")),
        "BAK must be created under the entry's parent directory; dirs: {:?}",
        dirs
    );
    assert!(
        !dirs.iter().any(|d| d.starts_with("syncroot/.kitchensync/")),
        "BAK must NOT be aggregated at the sync root; dirs: {:?}",
        dirs
    );
}

// ---------------------------------------------------------------------------
// 021.6: When rename fails, entry is left in place (LeftInPlace returned)
// 021.5: When rename fails, an error-level diagnostic is emitted
// ---------------------------------------------------------------------------

#[test]
fn rename_failure_returns_left_in_place_and_emits_diagnostic() {
    let subject = new();
    let tx = TestTransport::new();
    let out = TestOutput::new();
    let peer = transport::PeerHandle(0);
    tx.set_fail_rename();

    let outcome = subject.displace(&*tx, &*out, &peer, "some/parent", "file.txt", "ts-2026", false);

    assert!(
        matches!(outcome, DisplaceOutcome::LeftInPlace),
        "rename failure must return LeftInPlace so the walk continues without treating the entry as displaced"
    );
    assert!(
        !out.diagnostics().is_empty(),
        "an error diagnostic must be emitted when the rename into BAK fails"
    );
}
