use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant, SystemTime};

use copyqueue_copyscheduler::{
    new, CopyJob, CopyOutcome, CopyScheduler, SchedulerConfig, SchedulerObserver, SlotTrace,
};

// ---- Observer ----

struct ObsData {
    active: usize,
    peak: usize,
    acquired_per_src: HashMap<String, usize>,
    succeeded: usize,
    failed: usize,
}

struct Obs(Mutex<ObsData>);

impl Obs {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(ObsData {
            active: 0,
            peak: 0,
            acquired_per_src: HashMap::new(),
            succeeded: 0,
            failed: 0,
        })))
    }

    fn peak(&self) -> usize {
        self.0.lock().unwrap().peak
    }

    fn total_outcomes(&self) -> usize {
        let d = self.0.lock().unwrap();
        d.succeeded + d.failed
    }

    fn succeeded(&self) -> usize {
        self.0.lock().unwrap().succeeded
    }

    fn failed(&self) -> usize {
        self.0.lock().unwrap().failed
    }

    fn acquired_for(&self, src_path: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .acquired_per_src
            .get(src_path)
            .copied()
            .unwrap_or(0)
    }
}

impl SchedulerObserver for Obs {
    fn slot_trace(&self, copy: &CopyJob, trace: SlotTrace) {
        let mut d = self.0.lock().unwrap();
        match trace {
            SlotTrace::Acquired => {
                d.active += 1;
                *d.acquired_per_src
                    .entry(copy.src_path.clone())
                    .or_insert(0) += 1;
                if d.active > d.peak {
                    d.peak = d.active;
                }
            }
            SlotTrace::Released => {
                if d.active > 0 {
                    d.active -= 1;
                }
            }
        }
    }

    fn copy_outcome(&self, _copy: &CopyJob, outcome: CopyOutcome) {
        let mut d = self.0.lock().unwrap();
        match outcome {
            CopyOutcome::Succeeded => d.succeeded += 1,
            CopyOutcome::Failed => d.failed += 1,
        }
    }
}

// ---- Helpers ----

// Returns a CopyJob that will always fail: the source peer root does not exist.
fn failing_job(id: &str) -> CopyJob {
    CopyJob {
        src_peer: "file:///ks_cs_test_nonexistent_peer".to_string(),
        src_path: format!("ks_cs_missing_{}.dat", id),
        dst_peer: "file:///ks_cs_test_nonexistent_peer".to_string(),
        dst_path: format!("ks_cs_dst_{}.dat", id),
        mod_time: SystemTime::UNIX_EPOCH,
    }
}

// ---- Tests ----

// 020.1: when --max-copies is not given, at most 10 copies are active at one time.
#[test]
fn default_max_copies_is_ten() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: None,
            retries_copy: Some(1),
        },
        obs.clone(),
    );
    for i in 0..20 {
        sched.enqueue(failing_job(&i.to_string()));
    }
    sched.wait();
    assert!(
        obs.peak() <= 10,
        "peak active copies {} exceeded default limit of 10",
        obs.peak()
    );
    assert_eq!(
        obs.total_outcomes(),
        20,
        "every enqueued copy must report an outcome"
    );
}

// 020.2: at most --max-copies copies are active at one time.
#[test]
fn max_copies_limit_respected() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: Some(3),
            retries_copy: Some(1),
        },
        obs.clone(),
    );
    for i in 0..15 {
        sched.enqueue(failing_job(&i.to_string()));
    }
    sched.wait();
    assert!(
        obs.peak() <= 3,
        "peak active copies {} exceeded configured limit of 3",
        obs.peak()
    );
    assert_eq!(obs.total_outcomes(), 15);
}

// 020.3: a copy counts against the limit regardless of peer scheme.
#[test]
fn all_peer_schemes_count_against_slot_limit() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: Some(2),
            retries_copy: Some(1),
        },
        obs.clone(),
    );
    let jobs = vec![
        // file -> file
        CopyJob {
            src_peer: "file:///ks_cs_scheme_test".to_string(),
            src_path: "ff.dat".to_string(),
            dst_peer: "file:///ks_cs_scheme_test".to_string(),
            dst_path: "ff_dst.dat".to_string(),
            mod_time: SystemTime::UNIX_EPOCH,
        },
        // file -> sftp
        CopyJob {
            src_peer: "file:///ks_cs_scheme_test".to_string(),
            src_path: "fs.dat".to_string(),
            dst_peer: "sftp://127.0.0.1:9/ks_cs_scheme".to_string(),
            dst_path: "fs_dst.dat".to_string(),
            mod_time: SystemTime::UNIX_EPOCH,
        },
        // sftp -> file
        CopyJob {
            src_peer: "sftp://127.0.0.1:9/ks_cs_scheme".to_string(),
            src_path: "sf.dat".to_string(),
            dst_peer: "file:///ks_cs_scheme_test".to_string(),
            dst_path: "sf_dst.dat".to_string(),
            mod_time: SystemTime::UNIX_EPOCH,
        },
        // sftp -> sftp
        CopyJob {
            src_peer: "sftp://127.0.0.1:9/ks_cs_scheme_a".to_string(),
            src_path: "ss.dat".to_string(),
            dst_peer: "sftp://127.0.0.1:9/ks_cs_scheme_b".to_string(),
            dst_path: "ss_dst.dat".to_string(),
            mod_time: SystemTime::UNIX_EPOCH,
        },
    ];
    for job in jobs {
        sched.enqueue(job);
    }
    sched.wait();
    assert!(
        obs.peak() <= 2,
        "all peer schemes must count against the same slot limit; peak {} exceeded 2",
        obs.peak()
    );
    assert_eq!(
        obs.total_outcomes(),
        4,
        "every copy must report an outcome regardless of peer scheme"
    );
}

// 020.4: directory listing and other non-copy work proceeds even while copy slots are full.
#[test]
fn non_copy_work_not_blocked_by_copy_limit() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: Some(1),
            retries_copy: Some(1),
        },
        obs.clone(),
    );
    for i in 0..5 {
        sched.enqueue(failing_job(&format!("blk_{}", i)));
    }
    let ran = Arc::new(AtomicUsize::new(0));
    let ran2 = ran.clone();
    sched.submit(vec![Box::new(move || {
        ran2.fetch_add(1, Ordering::SeqCst);
    })]);
    sched.wait();
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "non-copy work must run even while copy slots are occupied"
    );
    assert_eq!(obs.total_outcomes(), 5);
}

// 020.5: newly enqueued copies are accepted while earlier copies are still running.
#[test]
fn incremental_enqueue_accepted_while_running() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: Some(2),
            retries_copy: Some(1),
        },
        obs.clone(),
    );
    // First batch: simulates copies queued when the first directory is scanned.
    for i in 0..5 {
        sched.enqueue(failing_job(&format!("b1_{}", i)));
    }
    // Second batch: simulates copies queued while the first batch may still be running.
    for i in 0..5 {
        sched.enqueue(failing_job(&format!("b2_{}", i)));
    }
    sched.wait();
    assert_eq!(
        obs.total_outcomes(),
        10,
        "all copies from both batches must reach a terminal outcome"
    );
}

// 020.6: per-peer directory listings for one directory level are issued concurrently.
#[test]
fn batch_non_copy_work_runs_concurrently() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: None,
            retries_copy: None,
        },
        obs.clone(),
    );

    let n = 4usize;
    let starts: Arc<Mutex<Vec<Instant>>> = Arc::new(Mutex::new(Vec::with_capacity(n)));
    let work: Vec<Box<dyn FnOnce() + Send + 'static>> = (0..n)
        .map(|_| {
            let s = starts.clone();
            Box::new(move || {
                s.lock().unwrap().push(Instant::now());
                std::thread::sleep(Duration::from_millis(60));
            }) as Box<dyn FnOnce() + Send + 'static>
        })
        .collect();

    sched.submit(work);
    sched.wait();

    let times = starts.lock().unwrap();
    assert_eq!(times.len(), n, "all submitted non-copy work items must run");
    let first = *times.iter().min().unwrap();
    let last = *times.iter().max().unwrap();
    // Concurrent: all items start before any finishes (spread << 60ms).
    // Sequential: items start 60ms apart (spread = 3 * 60ms = 180ms).
    assert!(
        last.duration_since(first) < Duration::from_millis(60),
        "batch non-copy items must run concurrently; start spread {:?} must be less than 60ms",
        last.duration_since(first)
    );
}

// 020.7 + 020.8: --retries-copy is the maximum total tries; default is 3.
#[test]
fn default_retries_copy_is_three() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: None,
            retries_copy: None,
        },
        obs.clone(),
    );
    sched.enqueue(failing_job("def_retry"));
    sched.wait();
    assert_eq!(obs.failed(), 1, "failing copy must be reported as Failed");
    assert_eq!(
        obs.acquired_for("ks_cs_missing_def_retry.dat"),
        3,
        "default retries-copy = 3 means exactly 3 slot-acquire events for a copy that always fails"
    );
}

#[test]
fn configured_retries_copy_is_total_try_count() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: None,
            retries_copy: Some(2),
        },
        obs.clone(),
    );
    sched.enqueue(failing_job("cfg_retry"));
    sched.wait();
    assert_eq!(obs.failed(), 1);
    assert_eq!(
        obs.acquired_for("ks_cs_missing_cfg_retry.dat"),
        2,
        "retries-copy = 2 means the copy is tried at most 2 times in total"
    );
}

// 020.9: a failed try below the limit requeues the copy; other queued work continues.
#[test]
fn failed_try_requeues_other_work_continues() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: Some(1),
            retries_copy: Some(3),
        },
        obs.clone(),
    );
    sched.enqueue(failing_job("req_a"));
    sched.enqueue(failing_job("req_b"));
    let ran = Arc::new(AtomicUsize::new(0));
    let ran2 = ran.clone();
    sched.submit(vec![Box::new(move || {
        ran2.fetch_add(1, Ordering::SeqCst);
    })]);
    sched.wait();
    assert_eq!(
        obs.failed(),
        2,
        "both copies must exhaust their tries and be marked Failed"
    );
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "non-copy work must complete while copies are being retried"
    );
    assert_eq!(
        obs.acquired_for("ks_cs_missing_req_a.dat"),
        3,
        "copy A must be tried 3 times"
    );
    assert_eq!(
        obs.acquired_for("ks_cs_missing_req_b.dat"),
        3,
        "copy B must be tried 3 times"
    );
}

// 020.10: a copy whose try count reaches the limit is marked failed and not requeued.
#[test]
fn copy_exhausting_tries_is_marked_failed_not_requeued() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: None,
            retries_copy: Some(2),
        },
        obs.clone(),
    );
    sched.enqueue(failing_job("exhaust"));
    sched.wait();
    assert_eq!(
        obs.failed(),
        1,
        "copy exhausting its try limit must be marked Failed"
    );
    assert_eq!(obs.succeeded(), 0);
    assert_eq!(
        obs.acquired_for("ks_cs_missing_exhaust.dat"),
        2,
        "copy must not be requeued beyond its try limit (2 tries max)"
    );
}

// 020.11: try budgets are tracked independently per copy.
#[test]
fn try_budgets_are_independent_per_copy() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: None,
            retries_copy: Some(3),
        },
        obs.clone(),
    );
    sched.enqueue(failing_job("indep_a"));
    sched.enqueue(failing_job("indep_b"));
    sched.wait();
    assert_eq!(
        obs.failed(),
        2,
        "both copies must be marked Failed with independent budgets"
    );
    assert_eq!(
        obs.acquired_for("ks_cs_missing_indep_a.dat"),
        3,
        "copy A must receive its full independent budget of 3 tries"
    );
    assert_eq!(
        obs.acquired_for("ks_cs_missing_indep_b.dat"),
        3,
        "copy B must receive its full independent budget of 3 tries"
    );
}

// 020.12: the try limit applies identically to local, SFTP, and mixed-scheme copies.
#[test]
fn try_limit_applies_identically_to_all_schemes() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: None,
            retries_copy: Some(2),
        },
        obs.clone(),
    );
    // file -> file (local)
    sched.enqueue(CopyJob {
        src_peer: "file:///ks_cs_012_nonexistent".to_string(),
        src_path: "local.dat".to_string(),
        dst_peer: "file:///ks_cs_012_nonexistent".to_string(),
        dst_path: "local_dst.dat".to_string(),
        mod_time: SystemTime::UNIX_EPOCH,
    });
    // sftp -> sftp
    sched.enqueue(CopyJob {
        src_peer: "sftp://127.0.0.1:9/ks_cs_012_sftp".to_string(),
        src_path: "remote.dat".to_string(),
        dst_peer: "sftp://127.0.0.1:9/ks_cs_012_sftp2".to_string(),
        dst_path: "remote_dst.dat".to_string(),
        mod_time: SystemTime::UNIX_EPOCH,
    });
    // file -> sftp (mixed)
    sched.enqueue(CopyJob {
        src_peer: "file:///ks_cs_012_nonexistent".to_string(),
        src_path: "mixed.dat".to_string(),
        dst_peer: "sftp://127.0.0.1:9/ks_cs_012_sftp3".to_string(),
        dst_path: "mixed_dst.dat".to_string(),
        mod_time: SystemTime::UNIX_EPOCH,
    });
    sched.wait();
    assert_eq!(
        obs.failed(),
        3,
        "all schemes must exhaust their try limit and be marked Failed"
    );
    assert_eq!(
        obs.acquired_for("local.dat"),
        2,
        "local copy must be tried exactly 2 times"
    );
    assert_eq!(
        obs.acquired_for("remote.dat"),
        2,
        "SFTP copy must be tried exactly 2 times"
    );
    assert_eq!(
        obs.acquired_for("mixed.dat"),
        2,
        "mixed-scheme copy must be tried exactly 2 times"
    );
}

// 024.7: --dry-run copies acquire copy slots subject to the global active-copy limit.
// The scheduler has no dry-run flag; it applies limits in all runs identically.
#[test]
fn copy_slot_limit_applies_in_dry_run() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: Some(2),
            retries_copy: Some(1),
        },
        obs.clone(),
    );
    for i in 0..10 {
        sched.enqueue(failing_job(&format!("dr7_{}", i)));
    }
    sched.wait();
    assert!(
        obs.peak() <= 2,
        "the copy slot limit must apply; dry-run mode is SwapTransfer's concern, not the scheduler's"
    );
    assert_eq!(obs.total_outcomes(), 10);
}

// 024.8: --dry-run applies the --retries-copy try limit to queued copies.
#[test]
fn retry_limit_applies_in_dry_run() {
    let sched = new();
    let obs = Obs::new();
    sched.configure(
        SchedulerConfig {
            max_copies: None,
            retries_copy: Some(2),
        },
        obs.clone(),
    );
    sched.enqueue(failing_job("dr8"));
    sched.wait();
    assert_eq!(
        obs.failed(),
        1,
        "the retry limit must apply regardless of dry-run mode"
    );
    assert_eq!(
        obs.acquired_for("ks_cs_missing_dr8.dat"),
        2,
        "dry-run copies must be tried at most retries-copy (2) times in total"
    );
}
