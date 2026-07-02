use std::any::Any;
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use copyqueue::{
    ConnectedPeerHandle, CopyFailurePhase, CopyInstallationState, CopyMutationPolicy, CopyQueue,
    CopyQueueEvent, CopyQueueRunRequest, PeerScheme, QueuedCopy, QueuedCopyOutcome,
    TransportErrorCategory,
};
use transportoperations_localtransportoperations::LocalTransportRoot;

fn subject() -> Arc<dyn CopyQueue> {
    let transport = transportoperations::new(
        transportoperations_localtransportoperations::new(),
        transportoperations_sftptransportoperations::new(),
    );

    let snapshot_database = snapshotstore_snapshotdatabase::new(
        snapshotstore_snapshotdatabase_snapshotcleanup::new(),
        snapshotstore_snapshotdatabase_snapshotfile::new(),
        snapshotstore_snapshotdatabase_snapshotrows::new(),
    );
    let snapshot_store = snapshotstore::new(
        snapshot_database.clone(),
        snapshotstore_snapshotidentity::new(),
        snapshotstore_snapshotpeerfiles::new(snapshot_database),
    );

    let staging_recovery = stagingrecovery::new(
        transport.clone(),
        stagingrecovery_bakdisplacement::new(),
        stagingrecovery_stagingcleanup::new(),
        stagingrecovery_swaprecovery::new(),
        stagingrecovery_tmpstagingpaths::new(),
    );

    let staged_transfer = copyqueue_stagedtransfer::new();
    let queue_runner = copyqueue_queuerunner::new(staged_transfer.clone());

    copyqueue::new(
        transport,
        snapshot_store,
        staging_recovery,
        queue_runner,
        staged_transfer,
    )
}

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kitchensync-copyqueue-tests-{}-{}",
        std::process::id(),
        name
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create test root");
    root
}

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, content).expect("write test file");
}

fn read_file(root: &Path, relative_path: &str) -> String {
    fs::read_to_string(root.join(relative_path)).expect("read test file")
}

fn local_peer(identity: &str, root: &Path) -> ConnectedPeerHandle {
    ConnectedPeerHandle {
        identity: identity.to_owned(),
        winning_url: format!("file://{}", root.to_string_lossy()),
        scheme: PeerScheme::File,
        handle: Arc::new(LocalTransportRoot {
            local_peer_root_path: root.to_path_buf(),
        }) as Arc<dyn Any + Send + Sync>,
    }
}

fn event_log() -> (Arc<Mutex<Vec<CopyQueueEvent>>>, copyqueue::CopyQueueEventSink) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink_events = events.clone();
    let sink = Arc::new(move |event| {
        sink_events.lock().expect("event log lock").push(event);
    });
    (events, sink)
}

fn queued_copy(source: &str, destination: &str, relpath: &str) -> QueuedCopy {
    QueuedCopy {
        source_peer_identity: "source".to_owned(),
        source_relative_file_path: source.to_owned(),
        destination_peer_identity: "destination".to_owned(),
        destination_relative_file_path: destination.to_owned(),
        report_relative_path: relpath.to_owned(),
        winning_mod_time: "1970-01-02_00-00-00_000000Z".to_owned(),
        winning_byte_size: 11,
    }
}

fn open_run(
    copy_queue: &dyn CopyQueue,
    max_active_copies: Option<u32>,
    max_total_tries: u32,
    source_root: &Path,
    destination_root: &Path,
    sink: copyqueue::CopyQueueEventSink,
) -> copyqueue::CopyQueueRunId {
    copy_queue
        .open_run(CopyQueueRunRequest {
            max_active_copies: max_active_copies.map(|value| {
                NonZeroU32::new(value).expect("test supplies nonzero max copies")
            }),
            max_total_tries_per_copy: NonZeroU32::new(max_total_tries)
                .expect("test supplies nonzero retry count"),
            peers: vec![
                local_peer("source", source_root),
                local_peer("destination", destination_root),
            ],
            mutation_policy: CopyMutationPolicy::Normal,
            event_sink: sink,
        })
        .expect("open copy queue run")
}

fn wait_for_event(events: &Arc<Mutex<Vec<CopyQueueEvent>>>, wanted: impl Fn(&CopyQueueEvent) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if events
            .lock()
            .expect("event log lock")
            .iter()
            .any(|event| wanted(event))
        {
            return;
        }

        assert!(Instant::now() < deadline, "timed out waiting for event");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn enqueued_copy_can_start_before_the_queue_is_closed_and_uses_default_slot_limit() {
    let source_root = temp_root("early-source");
    let destination_root = temp_root("early-destination");
    write_file(&source_root, "ready.txt", "hello world");

    let copy_queue = subject();
    let (events, sink) = event_log();
    let run_id = open_run(&*copy_queue, None, 1, &source_root, &destination_root, sink);

    copy_queue
        .enqueue(run_id, queued_copy("ready.txt", "ready.txt", "ready.txt"))
        .expect("enqueue copy");

    wait_for_event(&events, |event| {
        matches!(
            event,
            CopyQueueEvent::CopyStart {
                relpath,
                try_number: 1,
                ..
            } if relpath == "ready.txt"
        )
    });

    let result = copy_queue.close_and_drain(run_id).expect("drain run");

    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].outcome, QueuedCopyOutcome::Succeeded);
    assert!(events.lock().expect("event log lock").iter().any(|event| {
        matches!(event, CopyQueueEvent::CopySlotAcquire { active, max: 10 } if *active <= 10)
    }));
}

#[test]
fn configured_active_copy_limit_is_reported_and_never_exceeded() {
    let source_root = temp_root("limit-source");
    let destination_root = temp_root("limit-destination");
    for index in 0..4 {
        write_file(
            &source_root,
            &format!("copy-{index}.txt"),
            "hello world",
        );
    }

    let copy_queue = subject();
    let (events, sink) = event_log();
    let run_id = open_run(&*copy_queue, Some(2), 1, &source_root, &destination_root, sink);

    for index in 0..4 {
        let path = format!("copy-{index}.txt");
        copy_queue
            .enqueue(run_id, queued_copy(&path, &path, &path))
            .expect("enqueue copy");
    }

    let result = copy_queue.close_and_drain(run_id).expect("drain run");

    assert_eq!(result.results.len(), 4);
    assert!(result
        .results
        .iter()
        .all(|copy| copy.outcome == QueuedCopyOutcome::Succeeded));
    assert!(events.lock().expect("event log lock").iter().any(|event| {
        matches!(event, CopyQueueEvent::CopySlotAcquire { max: 2, .. })
    }));
    assert!(events.lock().expect("event log lock").iter().all(|event| {
        !matches!(event, CopyQueueEvent::CopySlotAcquire { active, max } if active > max)
    }));
}

#[test]
fn retryable_failure_moves_only_that_copy_behind_other_work_until_its_try_limit() {
    let source_root = temp_root("retry-source");
    let destination_root = temp_root("retry-destination");
    write_file(&source_root, "good.txt", "hello world");

    let copy_queue = subject();
    let (events, sink) = event_log();
    let run_id = open_run(&*copy_queue, Some(1), 2, &source_root, &destination_root, sink);

    copy_queue
        .enqueue(run_id, queued_copy("missing.txt", "missing.txt", "missing.txt"))
        .expect("enqueue missing copy");
    copy_queue
        .enqueue(run_id, queued_copy("good.txt", "good.txt", "good.txt"))
        .expect("enqueue good copy");

    let result = copy_queue.close_and_drain(run_id).expect("drain run");

    let starts: Vec<(String, u32)> = events
        .lock()
        .expect("event log lock")
        .iter()
        .filter_map(|event| match event {
            CopyQueueEvent::CopyStart {
                relpath,
                try_number,
                ..
            } => Some((relpath.clone(), *try_number)),
            _ => None,
        })
        .collect();
    assert_eq!(
        starts,
        vec![
            ("missing.txt".to_owned(), 1),
            ("good.txt".to_owned(), 1),
            ("missing.txt".to_owned(), 2),
        ]
    );

    let missing = result
        .results
        .iter()
        .find(|copy| copy.copy.report_relative_path == "missing.txt")
        .expect("missing copy result");
    assert_eq!(missing.total_tries, 2);
    assert_eq!(
        missing.outcome,
        QueuedCopyOutcome::FailedTryLimit {
            phase: CopyFailurePhase::ReadSource,
            transport_error: Some(TransportErrorCategory::NotFound),
            installation_state: CopyInstallationState::NotInstalled,
        }
    );

    let good = result
        .results
        .iter()
        .find(|copy| copy.copy.report_relative_path == "good.txt")
        .expect("good copy result");
    assert_eq!(good.total_tries, 1);
    assert_eq!(good.outcome, QueuedCopyOutcome::Succeeded);
}

#[test]
fn successful_replacement_uses_encoded_swap_paths_archives_old_and_removes_swap() {
    let source_root = temp_root("replace-source");
    let destination_root = temp_root("replace-destination");
    write_file(&source_root, "from/report%final.txt", "hello world");
    write_file(&destination_root, "to/report%final.txt", "old content");

    let copy_queue = subject();
    let (_events, sink) = event_log();
    let run_id = open_run(&*copy_queue, Some(1), 1, &source_root, &destination_root, sink);

    copy_queue
        .enqueue(
            run_id,
            queued_copy(
                "from/report%final.txt",
                "to/report%final.txt",
                "to/report%final.txt",
            ),
        )
        .expect("enqueue replacement");

    let result = copy_queue.close_and_drain(run_id).expect("drain run");

    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].outcome, QueuedCopyOutcome::Succeeded);
    assert_eq!(
        read_file(&destination_root, "to/report%final.txt"),
        "hello world"
    );
    assert!(!destination_root
        .join("to/.kitchensync/SWAP/report%25final.txt")
        .exists());

    let bak_root = destination_root.join("to/.kitchensync/BAK");
    let archived: Vec<PathBuf> = fs::read_dir(&bak_root)
        .expect("BAK timestamp directory exists")
        .map(|entry| {
            entry
                .expect("read BAK timestamp entry")
                .path()
                .join("report%final.txt")
        })
        .filter(|path| path.exists())
        .collect();

    assert_eq!(archived.len(), 1);
    assert_eq!(
        fs::read_to_string(&archived[0]).expect("read archived old file"),
        "old content"
    );
}
