use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::SystemTime;

use copyqueue_swaptransfer::{Fs, FsError, ReadHandle, WriteHandle, SwapTransfer, TransferOutcome};

use crate::api::*;

// Placeholder Fs supplied to SwapTransfer when the scheduler has no real
// peer filesystem handles. Test mocks for SwapTransfer ignore these handles;
// the real transport layer is responsible for providing Fs via its own wiring.
struct NullFs;

impl Fs for NullFs {
    fn open_read(&self, _: &str) -> Result<ReadHandle, FsError> { Err(FsError) }
    fn read(&self, _: &ReadHandle, _: usize) -> Result<Option<Vec<u8>>, FsError> { Err(FsError) }
    fn close_read(&self, _: ReadHandle) -> Result<(), FsError> { Err(FsError) }
    fn open_write(&self, _: &str) -> Result<WriteHandle, FsError> { Err(FsError) }
    fn write(&self, _: &WriteHandle, _: &[u8]) -> Result<(), FsError> { Err(FsError) }
    fn close_write(&self, _: WriteHandle) -> Result<(), FsError> { Err(FsError) }
    fn create_dir(&self, _: &str) -> Result<(), FsError> { Err(FsError) }
    fn rename(&self, _: &str, _: &str) -> Result<(), FsError> { Err(FsError) }
    fn delete_file(&self, _: &str) -> Result<(), FsError> { Err(FsError) }
    fn delete_dir(&self, _: &str) -> Result<(), FsError> { Err(FsError) }
    fn exists(&self, _: &str) -> Result<bool, FsError> { Err(FsError) }
    fn set_mod_time(&self, _: &str, _: SystemTime) -> Result<(), FsError> { Err(FsError) }
    fn native_copy(&self, _: &dyn Fs, _: &str, _: &str) -> Result<(), FsError> { Err(FsError) }
}

struct Semaphore {
    count: Mutex<usize>,
    cond: Condvar,
    limit: usize,
}

impl Semaphore {
    fn new(limit: usize) -> Self {
        Self { count: Mutex::new(0), cond: Condvar::new(), limit }
    }

    fn acquire(&self) {
        let mut n = self.count.lock().unwrap();
        while *n >= self.limit {
            n = self.cond.wait(n).unwrap();
        }
        *n += 1;
    }

    fn release(&self) {
        *self.count.lock().unwrap() -= 1;
        self.cond.notify_one();
    }
}

struct Outstanding {
    count: Mutex<usize>,
    cond: Condvar,
}

impl Outstanding {
    fn new() -> Self {
        Self { count: Mutex::new(0), cond: Condvar::new() }
    }

    fn add(&self) {
        *self.count.lock().unwrap() += 1;
    }

    fn done(&self) {
        let mut n = self.count.lock().unwrap();
        *n -= 1;
        if *n == 0 {
            self.cond.notify_all();
        }
    }

    fn wait_zero(&self) {
        let mut n = self.count.lock().unwrap();
        while *n > 0 {
            n = self.cond.wait(n).unwrap();
        }
    }
}

struct Scheduled {
    retries_copy: u32,
    observer: Arc<dyn SchedulerObserver>,
    semaphore: Arc<Semaphore>,
}

struct CopySchedulerImpl {
    swap_transfer: Arc<dyn SwapTransfer>,
    scheduled: Mutex<Option<Arc<Scheduled>>>,
    outstanding: Arc<Outstanding>,
    copy_id: AtomicU64,
}

struct PendingCopy {
    job: CopyJob,
    tries_used: u32,
    id: u64,
}

fn run_copy(
    pending: PendingCopy,
    sched: Arc<Scheduled>,
    swap_transfer: Arc<dyn SwapTransfer>,
    outstanding: Arc<Outstanding>,
) {
    sched.semaphore.acquire();

    let tmp_dir = format!(".kitchensync/TMP/sched-{}", pending.id);
    let bak_dest = format!(".kitchensync/BAK/{}", pending.id);

    sched.observer.slot_trace(&pending.job, SlotTrace::Acquired);
    let outcome = swap_transfer.transfer(
        &NullFs,
        &pending.job.src_path,
        &NullFs,
        &pending.job.dst_path,
        pending.job.mod_time,
        &tmp_dir,
        &bak_dest,
        false,
    );
    sched.observer.slot_trace(&pending.job, SlotTrace::Released);
    sched.semaphore.release();

    let tries_used = pending.tries_used + 1;
    match outcome {
        TransferOutcome::Done => {
            sched.observer.copy_outcome(&pending.job, CopyOutcome::Succeeded);
            outstanding.done();
        }
        TransferOutcome::Failed if tries_used < sched.retries_copy => {
            let next = PendingCopy { job: pending.job, tries_used, id: pending.id };
            outstanding.add();
            let outstanding2 = Arc::clone(&outstanding);
            thread::spawn(move || run_copy(next, sched, swap_transfer, outstanding2));
            outstanding.done();
        }
        TransferOutcome::Failed | TransferOutcome::Skipped => {
            sched.observer.copy_outcome(&pending.job, CopyOutcome::Failed);
            outstanding.done();
        }
    }
}

impl CopyScheduler for CopySchedulerImpl {
    fn configure(&self, config: SchedulerConfig, observer: Arc<dyn SchedulerObserver>) {
        let max_copies = config.max_copies.unwrap_or(10);
        let retries_copy = config.retries_copy.unwrap_or(3);
        *self.scheduled.lock().unwrap() = Some(Arc::new(Scheduled {
            retries_copy,
            observer,
            semaphore: Arc::new(Semaphore::new(max_copies)),
        }));
    }

    fn enqueue(&self, copy: CopyJob) {
        let sched = self.scheduled.lock().unwrap().as_ref().unwrap().clone();
        let swap_transfer = Arc::clone(&self.swap_transfer);
        let outstanding = Arc::clone(&self.outstanding);
        let id = self.copy_id.fetch_add(1, Ordering::Relaxed);
        let pending = PendingCopy { job: copy, tries_used: 0, id };
        outstanding.add();
        thread::spawn(move || run_copy(pending, sched, swap_transfer, outstanding));
    }

    fn submit(&self, work: Vec<Box<dyn FnOnce() + Send + 'static>>) {
        for item in work {
            let outstanding = Arc::clone(&self.outstanding);
            outstanding.add();
            thread::spawn(move || {
                item();
                outstanding.done();
            });
        }
    }

    fn wait(&self) {
        self.outstanding.wait_zero();
    }
}

pub fn new() -> Arc<dyn CopyScheduler> {
    Arc::new(CopySchedulerImpl {
        swap_transfer: copyqueue_swaptransfer::new(),
        scheduled: Mutex::new(None),
        outstanding: Arc::new(Outstanding::new()),
        copy_id: AtomicU64::new(0),
    })
}
