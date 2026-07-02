use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

use copyqueue_stagedtransfer::{
    new, StagedTransferFileOperations, StagedTransferModificationTime, StagedTransferOperationError,
    StagedTransferPeer, StagedTransferRequest, StagedTransferSwapOldState,
    StagedTransferSwapRecovery, StagedTransferTimestampGenerator, StagedTransferTransportErrorCategory,
    StagedTransferTryOutcome,
};

#[test]
fn replaces_existing_file_through_swap_old_then_archives_and_cleans_swap() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    let timestamps = FixedTimestamps::new(&ops, vec!["2026-07-02T10-40-49Z"]);
    let request = request("incoming/source.bin", "folder/weird%\\name.txt", 123, 456);

    ops.put(&request.source_peer, "incoming/source.bin", b"replacement");
    ops.put(&request.destination_peer, "folder/weird%\\name.txt", b"original");

    let outcome =
        subject.run_transfer_try(request.clone(), &ops, &recovery, &timestamps);

    assert_eq!(outcome, StagedTransferTryOutcome::Success);
    assert_eq!(
        ops.bytes(&request.destination_peer, "folder/weird%\\name.txt"),
        Some(b"replacement".to_vec())
    );
    assert_eq!(
        ops.bytes(
            &request.destination_peer,
            "folder/.kitchensync/BAK/2026-07-02T10-40-49Z/weird%\\name.txt"
        ),
        Some(b"original".to_vec())
    );
    assert_eq!(
        ops.modification_time(&request.destination_peer, "folder/weird%\\name.txt"),
        Some(request.winning_modification_time)
    );
    assert_eq!(timestamps.calls(), 1);
    assert_eq!(
        recovery.calls(),
        vec![RecoveryCall {
            peer: request.destination_peer.clone(),
            target_parent_path: "folder".to_string(),
            basename: "weird%\\name.txt".to_string(),
            encoded_basename: "weird%25%5Cname.txt".to_string(),
        }]
    );

    let events = ops.events();
    assert!(
        event_index(
            &events,
            "write:dest:folder/.kitchensync/SWAP/weird%25%5Cname.txt/new:11"
        ) < event_index(
            &events,
            "rename:dest:folder/.kitchensync/SWAP/weird%25%5Cname.txt/new->folder/weird%\\name.txt"
        ),
        "replacement bytes must be written to SWAP new before the final rename"
    );
    assert_subsequence(
        &events,
        &[
            "recover:folder:weird%\\name.txt:weird%25%5Cname.txt",
            "mkdir:dest:folder/.kitchensync/SWAP/weird%25%5Cname.txt",
            "create_new:dest:folder/.kitchensync/SWAP/weird%25%5Cname.txt/new",
            "exists:dest:folder/weird%\\name.txt",
            "rename:dest:folder/weird%\\name.txt->folder/.kitchensync/SWAP/weird%25%5Cname.txt/old",
            "rename:dest:folder/.kitchensync/SWAP/weird%25%5Cname.txt/new->folder/weird%\\name.txt",
            "mtime:dest:folder/weird%\\name.txt",
            "timestamp",
            "mkdir:dest:folder/.kitchensync/BAK/2026-07-02T10-40-49Z",
            "rename:dest:folder/.kitchensync/SWAP/weird%25%5Cname.txt/old->folder/.kitchensync/BAK/2026-07-02T10-40-49Z/weird%\\name.txt",
            "rmdir:dest:folder/.kitchensync/SWAP/weird%25%5Cname.txt",
            "rmdir:dest:folder/.kitchensync/SWAP",
        ],
    );
    assert!(
        !events
            .iter()
            .any(|event| event == "create_new:dest:folder/weird%\\name.txt"),
        "replacement content must not be written directly to the final path"
    );
}

#[test]
fn first_time_destination_uses_swap_new_and_creates_no_bak_entry() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    let timestamps = FixedTimestamps::new(&ops, vec!["unused"]);
    let request = request("src.txt", "created.txt", 1, 0);

    ops.put(&request.source_peer, "src.txt", b"new file");

    let outcome =
        subject.run_transfer_try(request.clone(), &ops, &recovery, &timestamps);

    assert_eq!(outcome, StagedTransferTryOutcome::Success);
    assert_eq!(
        ops.bytes(&request.destination_peer, "created.txt"),
        Some(b"new file".to_vec())
    );
    assert!(ops.paths(&request.destination_peer).iter().all(|path| {
        !path.starts_with(".kitchensync/BAK/")
    }));
    assert_eq!(timestamps.calls(), 0);
    assert_subsequence(
        &ops.events(),
        &[
            "recover::created.txt:created.txt",
            "create_new:dest:.kitchensync/SWAP/created.txt/new",
            "exists:dest:created.txt",
            "rename:dest:.kitchensync/SWAP/created.txt/new->created.txt",
            "mtime:dest:created.txt",
            "rmdir:dest:.kitchensync/SWAP/created.txt",
            "rmdir:dest:.kitchensync/SWAP",
        ],
    );
}

#[test]
fn failed_move_to_swap_old_skips_copy_leaves_original_and_deletes_swap_new() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    let timestamps = FixedTimestamps::new(&ops, vec!["unused"]);
    let request = request("source.txt", "target.txt", 2, 0);

    ops.put(&request.source_peer, "source.txt", b"replacement");
    ops.put(&request.destination_peer, "target.txt", b"original");
    ops.fail_rename("target.txt", ".kitchensync/SWAP/target.txt/old");

    let outcome =
        subject.run_transfer_try(request.clone(), &ops, &recovery, &timestamps);

    match outcome {
        StagedTransferTryOutcome::SkipRestOfRun(failure) => {
            assert_eq!(
                failure.phase,
                copyqueue_stagedtransfer::StagedTransferFailurePhase::MoveExistingToSwapOld
            );
            assert_eq!(failure.swap_old_state, StagedTransferSwapOldState::NotCreated);
        }
        other => panic!("expected skip result, got {other:?}"),
    }
    assert_eq!(
        ops.bytes(&request.destination_peer, "target.txt"),
        Some(b"original".to_vec())
    );
    assert_eq!(
        ops.bytes(&request.destination_peer, ".kitchensync/SWAP/target.txt/new"),
        None
    );
    assert!(
        !ops.events()
            .iter()
            .any(|event| event == "rename:dest:.kitchensync/SWAP/target.txt/new->target.txt")
    );
}

#[test]
fn failure_before_swap_old_deletes_swap_new_and_reports_failed_phase() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    let timestamps = FixedTimestamps::new(&ops, vec!["unused"]);
    let request = request("source.txt", "missing-parent/target.txt", 3, 0);

    ops.put(&request.source_peer, "source.txt", b"replacement");
    ops.fail_rename(
        "missing-parent/.kitchensync/SWAP/target.txt/new",
        "missing-parent/target.txt",
    );

    let outcome =
        subject.run_transfer_try(request.clone(), &ops, &recovery, &timestamps);

    match outcome {
        StagedTransferTryOutcome::Failure(failure) => {
            assert_eq!(
                failure.phase,
                copyqueue_stagedtransfer::StagedTransferFailurePhase::RenameFinal
            );
            assert_eq!(failure.swap_old_state, StagedTransferSwapOldState::NotCreated);
        }
        other => panic!("expected failure result, got {other:?}"),
    }
    assert_eq!(
        ops.bytes(
            &request.destination_peer,
            "missing-parent/.kitchensync/SWAP/target.txt/new"
        ),
        None
    );
    assert_subsequence(
        &ops.events(),
        &[
            "rename:dest:missing-parent/.kitchensync/SWAP/target.txt/new->missing-parent/target.txt",
            "delete:dest:missing-parent/.kitchensync/SWAP/target.txt/new",
        ],
    );
}

#[test]
fn failure_after_swap_old_leaves_peer_visible_incomplete_replacement_state() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    let timestamps = FixedTimestamps::new(&ops, vec!["unused"]);
    let request = request("source.txt", "target.txt", 4, 0);

    ops.put(&request.source_peer, "source.txt", b"replacement");
    ops.put(&request.destination_peer, "target.txt", b"original");
    ops.fail_rename(".kitchensync/SWAP/target.txt/new", "target.txt");

    let outcome =
        subject.run_transfer_try(request.clone(), &ops, &recovery, &timestamps);

    match outcome {
        StagedTransferTryOutcome::Failure(failure) => {
            assert_eq!(
                failure.phase,
                copyqueue_stagedtransfer::StagedTransferFailurePhase::RenameFinal
            );
            assert_eq!(failure.swap_old_state, StagedTransferSwapOldState::Created);
        }
        other => panic!("expected failure result, got {other:?}"),
    }
    assert_eq!(ops.bytes(&request.destination_peer, "target.txt"), None);
    assert_eq!(
        ops.bytes(&request.destination_peer, ".kitchensync/SWAP/target.txt/old"),
        Some(b"original".to_vec())
    );
    assert_eq!(
        ops.bytes(&request.destination_peer, ".kitchensync/SWAP/target.txt/new"),
        Some(b"replacement".to_vec())
    );
}

#[test]
fn recovery_failure_stops_before_any_replacement_write() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    recovery.fail();
    let timestamps = FixedTimestamps::new(&ops, vec!["unused"]);
    let request = request("source.txt", "target.txt", 5, 0);

    ops.put(&request.source_peer, "source.txt", b"replacement");

    let outcome =
        subject.run_transfer_try(request.clone(), &ops, &recovery, &timestamps);

    match outcome {
        StagedTransferTryOutcome::RecoveryFailure(error) => {
            assert_eq!(error.message, "recovery failed");
        }
        other => panic!("expected recovery failure, got {other:?}"),
    }
    assert_eq!(
        ops.events(),
        vec!["recover::target.txt:target.txt".to_string()]
    );
    assert_eq!(ops.bytes(&request.destination_peer, "target.txt"), None);
}

#[test]
fn source_read_failure_reports_read_source_and_leaves_no_swap_new() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    let timestamps = FixedTimestamps::new(&ops, vec!["unused"]);
    let request = request("missing-source.txt", "target.txt", 6, 0);

    let outcome =
        subject.run_transfer_try(request.clone(), &ops, &recovery, &timestamps);

    match outcome {
        StagedTransferTryOutcome::Failure(failure) => {
            assert_eq!(
                failure.phase,
                copyqueue_stagedtransfer::StagedTransferFailurePhase::ReadSource
            );
            assert_eq!(failure.swap_old_state, StagedTransferSwapOldState::NotCreated);
        }
        other => panic!("expected read-source failure, got {other:?}"),
    }
    assert_eq!(ops.bytes(&request.destination_peer, "target.txt"), None);
    assert_eq!(
        ops.bytes(&request.destination_peer, ".kitchensync/SWAP/target.txt/new"),
        None
    );
}

#[test]
fn swap_new_write_failure_reports_write_swap_new_before_replacement() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    let timestamps = FixedTimestamps::new(&ops, vec!["unused"]);
    let request = request("source.txt", "target.txt", 7, 0);

    ops.put(&request.source_peer, "source.txt", b"replacement");
    ops.fail_create_new(".kitchensync/SWAP/target.txt/new");

    let outcome =
        subject.run_transfer_try(request.clone(), &ops, &recovery, &timestamps);

    match outcome {
        StagedTransferTryOutcome::Failure(failure) => {
            assert_eq!(
                failure.phase,
                copyqueue_stagedtransfer::StagedTransferFailurePhase::WriteSwapNew
            );
            assert_eq!(failure.swap_old_state, StagedTransferSwapOldState::NotCreated);
        }
        other => panic!("expected write-swap-new failure, got {other:?}"),
    }
    assert_eq!(ops.bytes(&request.destination_peer, "target.txt"), None);
}

#[test]
fn modification_time_failure_reports_set_mod_time_without_undoing_replacement() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    let timestamps = FixedTimestamps::new(&ops, vec!["unused"]);
    let request = request("source.txt", "target.txt", 8, 0);

    ops.put(&request.source_peer, "source.txt", b"replacement");
    ops.fail_set_modification_time("target.txt");

    let outcome =
        subject.run_transfer_try(request.clone(), &ops, &recovery, &timestamps);

    match outcome {
        StagedTransferTryOutcome::Failure(failure) => {
            assert_eq!(
                failure.phase,
                copyqueue_stagedtransfer::StagedTransferFailurePhase::SetModTime
            );
            assert_eq!(failure.swap_old_state, StagedTransferSwapOldState::NotCreated);
        }
        other => panic!("expected set-mod-time failure, got {other:?}"),
    }
    assert_eq!(
        ops.bytes(&request.destination_peer, "target.txt"),
        Some(b"replacement".to_vec())
    );
}

#[test]
fn archive_failure_reports_archive_old_and_leaves_swap_old_for_recovery() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    let timestamps = FixedTimestamps::new(&ops, vec!["2026-07-02T10-41-00Z"]);
    let request = request("source.txt", "target.txt", 9, 0);

    ops.put(&request.source_peer, "source.txt", b"replacement");
    ops.put(&request.destination_peer, "target.txt", b"original");
    ops.fail_rename(
        ".kitchensync/SWAP/target.txt/old",
        ".kitchensync/BAK/2026-07-02T10-41-00Z/target.txt",
    );

    let outcome =
        subject.run_transfer_try(request.clone(), &ops, &recovery, &timestamps);

    match outcome {
        StagedTransferTryOutcome::Failure(failure) => {
            assert_eq!(
                failure.phase,
                copyqueue_stagedtransfer::StagedTransferFailurePhase::ArchiveOld
            );
            assert_eq!(failure.swap_old_state, StagedTransferSwapOldState::Created);
        }
        other => panic!("expected archive-old failure, got {other:?}"),
    }
    assert_eq!(
        ops.bytes(&request.destination_peer, "target.txt"),
        Some(b"replacement".to_vec())
    );
    assert_eq!(
        ops.bytes(&request.destination_peer, ".kitchensync/SWAP/target.txt/old"),
        Some(b"original".to_vec())
    );
}

#[test]
fn cleanup_failure_reports_cleanup_after_replacement_and_archive_work_succeeded() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    let timestamps = FixedTimestamps::new(&ops, vec!["2026-07-02T10-41-01Z"]);
    let request = request("source.txt", "target.txt", 10, 0);

    ops.put(&request.source_peer, "source.txt", b"replacement");
    ops.put(&request.destination_peer, "target.txt", b"original");
    ops.fail_remove_empty_directory(".kitchensync/SWAP/target.txt");

    let outcome =
        subject.run_transfer_try(request.clone(), &ops, &recovery, &timestamps);

    match outcome {
        StagedTransferTryOutcome::Failure(failure) => {
            assert_eq!(
                failure.phase,
                copyqueue_stagedtransfer::StagedTransferFailurePhase::Cleanup
            );
            assert_eq!(failure.swap_old_state, StagedTransferSwapOldState::Created);
        }
        other => panic!("expected cleanup failure, got {other:?}"),
    }
    assert_eq!(
        ops.bytes(&request.destination_peer, "target.txt"),
        Some(b"replacement".to_vec())
    );
    assert_eq!(
        ops.bytes(
            &request.destination_peer,
            ".kitchensync/BAK/2026-07-02T10-41-01Z/target.txt"
        ),
        Some(b"original".to_vec())
    );
}

#[test]
fn destination_writing_starts_before_the_source_is_fully_read() {
    let subject = new();
    let ops = MemoryPeerOps::new();
    let recovery = RecordingRecovery::new(&ops);
    let timestamps = FixedTimestamps::new(&ops, vec!["unused"]);
    let large_request = request("large.bin", "large-copy.bin", 11, 131_072);
    let small_request = request("small.bin", "small-copy.bin", 12, 1);

    ops.put(&large_request.source_peer, "large.bin", &vec![b'x'; 131_072]);
    ops.put(&small_request.source_peer, "small.bin", b"x");

    let outcome =
        subject.run_transfer_try(large_request.clone(), &ops, &recovery, &timestamps);
    let small_outcome =
        subject.run_transfer_try(small_request.clone(), &ops, &recovery, &timestamps);

    assert_eq!(outcome, StagedTransferTryOutcome::Success);
    assert_eq!(small_outcome, StagedTransferTryOutcome::Success);
    let events = ops.events();
    let first_write = event_index_with_prefix(
        &events,
        "write:dest:.kitchensync/SWAP/large-copy.bin/new:",
    );
    let eof_read = event_index(&events, "read:source:large.bin:0");
    assert!(
        first_write < eof_read,
        "destination SWAP new must receive bytes before source EOF"
    );
    assert_eq!(
        ops.max_read_buffer_len(&large_request.source_peer, "large.bin"),
        ops.max_read_buffer_len(&small_request.source_peer, "small.bin"),
        "read buffer capacity must be independent of copied file size"
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecoveryCall {
    peer: StagedTransferPeer,
    target_parent_path: String,
    basename: String,
    encoded_basename: String,
}

struct RecordingRecovery {
    state: Arc<Mutex<MemoryState>>,
    calls: Mutex<Vec<RecoveryCall>>,
    fail: Mutex<bool>,
}

impl RecordingRecovery {
    fn new(ops: &MemoryPeerOps) -> Self {
        Self {
            state: ops.event_state(),
            calls: Mutex::new(Vec::new()),
            fail: Mutex::new(false),
        }
    }

    fn fail(&self) {
        *self.fail.lock().unwrap() = true;
    }

    fn calls(&self) -> Vec<RecoveryCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl StagedTransferSwapRecovery for RecordingRecovery {
    fn recover_user_data_swap(
        &self,
        peer: &StagedTransferPeer,
        target_parent_path: &str,
        basename: &str,
        encoded_basename: &str,
    ) -> Result<(), StagedTransferOperationError> {
        self.calls.lock().unwrap().push(RecoveryCall {
            peer: peer.clone(),
            target_parent_path: target_parent_path.to_string(),
            basename: basename.to_string(),
            encoded_basename: encoded_basename.to_string(),
        });
        self.state.lock().unwrap().events.push(format!(
            "recover:{target_parent_path}:{basename}:{encoded_basename}"
        ));
        if *self.fail.lock().unwrap() {
            Err(operation_error("recovery failed"))
        } else {
            Ok(())
        }
    }
}

struct FixedTimestamps {
    state: Arc<Mutex<MemoryState>>,
    values: Mutex<Vec<String>>,
    calls: Mutex<usize>,
}

impl FixedTimestamps {
    fn new(ops: &MemoryPeerOps, values: Vec<&str>) -> Self {
        Self {
            state: ops.event_state(),
            values: Mutex::new(values.into_iter().map(String::from).rev().collect()),
            calls: Mutex::new(0),
        }
    }

    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl StagedTransferTimestampGenerator for FixedTimestamps {
    fn next_bak_timestamp(&self) -> String {
        *self.calls.lock().unwrap() += 1;
        self.state.lock().unwrap().events.push("timestamp".to_string());
        self.values
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| "timestamp".to_string())
    }
}

struct MemoryPeerOps {
    state: Arc<Mutex<MemoryState>>,
}

#[derive(Default)]
struct MemoryState {
    files: HashMap<(String, String), Vec<u8>>,
    modification_times: HashMap<(String, String), StagedTransferModificationTime>,
    directories: Vec<(String, String)>,
    events: Vec<String>,
    failing_renames: Vec<(String, String)>,
    failing_creates: Vec<String>,
    failing_modification_times: Vec<String>,
    failing_remove_directories: Vec<String>,
    read_buffer_lengths: Vec<(String, String, usize)>,
}

impl MemoryPeerOps {
    fn new() -> Self {
        let state = Arc::new(Mutex::new(MemoryState::default()));
        Self { state }
    }

    fn event_state(&self) -> Arc<Mutex<MemoryState>> {
        self.state.clone()
    }

    fn put(&self, peer: &StagedTransferPeer, path: &str, bytes: &[u8]) {
        self.state
            .lock()
            .unwrap()
            .files
            .insert((peer.id.clone(), path.to_string()), bytes.to_vec());
    }

    fn bytes(&self, peer: &StagedTransferPeer, path: &str) -> Option<Vec<u8>> {
        self.state
            .lock()
            .unwrap()
            .files
            .get(&(peer.id.clone(), path.to_string()))
            .cloned()
    }

    fn paths(&self, peer: &StagedTransferPeer) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .files
            .keys()
            .filter(|(peer_id, _)| peer_id == &peer.id)
            .map(|(_, path)| path.clone())
            .collect()
    }

    fn modification_time(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Option<StagedTransferModificationTime> {
        self.state
            .lock()
            .unwrap()
            .modification_times
            .get(&(peer.id.clone(), path.to_string()))
            .copied()
    }

    fn fail_rename(&self, source_path: &str, destination_path: &str) {
        self.state.lock().unwrap().failing_renames.push((
            source_path.to_string(),
            destination_path.to_string(),
        ));
    }

    fn fail_create_new(&self, path: &str) {
        self.state
            .lock()
            .unwrap()
            .failing_creates
            .push(path.to_string());
    }

    fn fail_set_modification_time(&self, path: &str) {
        self.state
            .lock()
            .unwrap()
            .failing_modification_times
            .push(path.to_string());
    }

    fn fail_remove_empty_directory(&self, path: &str) {
        self.state
            .lock()
            .unwrap()
            .failing_remove_directories
            .push(path.to_string());
    }

    fn events(&self) -> Vec<String> {
        self.state.lock().unwrap().events.clone()
    }

    fn max_read_buffer_len(&self, peer: &StagedTransferPeer, path: &str) -> Option<usize> {
        self.state
            .lock()
            .unwrap()
            .read_buffer_lengths
            .iter()
            .filter(|(peer_id, read_path, _)| peer_id == &peer.id && read_path == path)
            .map(|(_, _, length)| *length)
            .max()
    }
}

impl StagedTransferFileOperations for MemoryPeerOps {
    fn file_exists(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<bool, StagedTransferOperationError> {
        let mut state = self.state.lock().unwrap();
        state.events.push(format!("exists:{}:{path}", peer.id));
        Ok(state.files.contains_key(&(peer.id.clone(), path.to_string())))
    }

    fn open_for_read(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<Box<dyn Read + Send>, StagedTransferOperationError> {
        let mut state = self.state.lock().unwrap();
        state.events.push(format!("open_read:{}:{path}", peer.id));
        let bytes = state
            .files
            .get(&(peer.id.clone(), path.to_string()))
            .cloned()
            .ok_or_else(|| operation_error("source missing"))?;
        Ok(Box::new(RecordingReader {
            state: self.state.clone(),
            peer_id: peer.id.clone(),
            path: path.to_string(),
            bytes,
            offset: 0,
        }))
    }

    fn create_new_for_write(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<Box<dyn Write + Send>, StagedTransferOperationError> {
        let mut state = self.state.lock().unwrap();
        state.events.push(format!("create_new:{}:{path}", peer.id));
        if state.failing_creates.iter().any(|failing| failing == path) {
            return Err(operation_error("create failed"));
        }
        let key = (peer.id.clone(), path.to_string());
        if state.files.contains_key(&key) {
            return Err(operation_error("destination already exists"));
        }
        state.files.insert(key.clone(), Vec::new());
        Ok(Box::new(RecordingWriter {
            state: self.state.clone(),
            key,
        }))
    }

    fn create_directory_all(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<(), StagedTransferOperationError> {
        let mut state = self.state.lock().unwrap();
        state.events.push(format!("mkdir:{}:{path}", peer.id));
        state.directories.push((peer.id.clone(), path.to_string()));
        Ok(())
    }

    fn rename_to_missing_path(
        &self,
        peer: &StagedTransferPeer,
        source_path: &str,
        destination_path: &str,
    ) -> Result<(), StagedTransferOperationError> {
        let mut state = self.state.lock().unwrap();
        state.events.push(format!(
            "rename:{}:{source_path}->{destination_path}",
            peer.id
        ));
        if state
            .failing_renames
            .iter()
            .any(|(source, destination)| source == source_path && destination == destination_path)
        {
            return Err(operation_error("rename failed"));
        }
        let source_key = (peer.id.clone(), source_path.to_string());
        let destination_key = (peer.id.clone(), destination_path.to_string());
        if state.files.contains_key(&destination_key) {
            return Err(operation_error("destination exists"));
        }
        let bytes = state
            .files
            .remove(&source_key)
            .ok_or_else(|| operation_error("source missing"))?;
        state.files.insert(destination_key, bytes);
        Ok(())
    }

    fn delete_file(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<(), StagedTransferOperationError> {
        let mut state = self.state.lock().unwrap();
        state.events.push(format!("delete:{}:{path}", peer.id));
        state.files.remove(&(peer.id.clone(), path.to_string()));
        Ok(())
    }

    fn remove_empty_directory(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
    ) -> Result<(), StagedTransferOperationError> {
        let mut state = self.state.lock().unwrap();
        state.events.push(format!("rmdir:{}:{path}", peer.id));
        if state
            .failing_remove_directories
            .iter()
            .any(|failing| failing == path)
        {
            return Err(operation_error("cleanup failed"));
        }
        Ok(())
    }

    fn set_modification_time(
        &self,
        peer: &StagedTransferPeer,
        path: &str,
        modification_time: StagedTransferModificationTime,
    ) -> Result<(), StagedTransferOperationError> {
        let mut state = self.state.lock().unwrap();
        state.events.push(format!("mtime:{}:{path}", peer.id));
        if state
            .failing_modification_times
            .iter()
            .any(|failing| failing == path)
        {
            return Err(operation_error("mtime failed"));
        }
        state.modification_times.insert(
            (peer.id.clone(), path.to_string()),
            modification_time,
        );
        Ok(())
    }
}

struct RecordingReader {
    state: Arc<Mutex<MemoryState>>,
    peer_id: String,
    path: String,
    bytes: Vec<u8>,
    offset: usize,
}

impl Read for RecordingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.bytes.len().saturating_sub(self.offset);
        let count = available.min(buffer.len());
        if count > 0 {
            buffer[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
            self.offset += count;
        }
        let mut state = self.state.lock().unwrap();
        state.read_buffer_lengths.push((
            self.peer_id.clone(),
            self.path.clone(),
            buffer.len(),
        ));
        state.events.push(format!(
            "read:{}:{}:{count}",
            self.peer_id, self.path
        ));
        Ok(count)
    }
}

struct RecordingWriter {
    state: Arc<Mutex<MemoryState>>,
    key: (String, String),
}

impl Write for RecordingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.state.lock().unwrap();
        state.events.push(format!(
            "write:{}:{}:{}",
            self.key.0,
            self.key.1,
            bytes.len()
        ));
        state
            .files
            .get_mut(&self.key)
            .expect("writer target exists")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        state
            .events
            .push(format!("flush:{}:{}", self.key.0, self.key.1));
        Ok(())
    }
}

fn request(
    source_path: &str,
    destination_path: &str,
    seconds: i64,
    winning_byte_size: u64,
) -> StagedTransferRequest {
    StagedTransferRequest {
        source_peer: StagedTransferPeer {
            id: "source".to_string(),
        },
        destination_peer: StagedTransferPeer {
            id: "dest".to_string(),
        },
        relative_source_file_path: source_path.to_string(),
        relative_destination_file_path: destination_path.to_string(),
        user_path: destination_path.to_string(),
        winning_modification_time: StagedTransferModificationTime {
            seconds_since_unix_epoch: seconds,
            nanoseconds: 987,
        },
        winning_byte_size,
    }
}

fn operation_error(message: &str) -> StagedTransferOperationError {
    StagedTransferOperationError {
        transport_error_category: Some(StagedTransferTransportErrorCategory::IoError),
        message: message.to_string(),
    }
}

fn assert_subsequence(events: &[String], expected: &[&str]) {
    let mut cursor = 0;
    for expected_event in expected {
        if let Some(offset) = events[cursor..]
            .iter()
            .position(|event| event == expected_event)
        {
            cursor += offset + 1;
        } else {
            panic!("missing event {expected_event:?} in {events:?}");
        }
    }
}

fn event_index(events: &[String], expected: &str) -> usize {
    events
        .iter()
        .position(|event| event == expected)
        .unwrap_or_else(|| panic!("missing event {expected:?} in {events:?}"))
}

fn event_index_with_prefix(events: &[String], expected_prefix: &str) -> usize {
    events
        .iter()
        .position(|event| event.starts_with(expected_prefix))
        .unwrap_or_else(|| panic!("missing event prefix {expected_prefix:?} in {events:?}"))
}
