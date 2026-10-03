//! The file-copy queue and the SWAP-based transfer procedure.
//! See specs/sync.md ("File Copy", "Rename Compatibility") and
//! specs/concurrency.md ("Copy Concurrency", "Copy Queue Tries").

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use crate::output;
use crate::transport::{join, Result, Transport};
use crate::util::{basename, micros_to_system, now_string, parent_path};

use super::fsops::{self, kind_name, meta_dir, swap_dir_for, PeerRef};
use super::peer::{note_failure, DirState};
use super::reuse::Reuse;

pub struct CopyJob {
    pub src: PeerRef,
    pub dst: PeerRef,
    /// Manifest state of the destination directory, told when we finish.
    pub dst_dir: Arc<DirState>,
    pub path: String,
    pub mod_time: i64,
    pub byte_size: i64,
    /// Held until the walk ends, then allowed to reuse a displaced file.
    pub held: bool,
    pub tries: u32,
}

struct State {
    queue: VecDeque<CopyJob>,
    /// Held copies, in the order they were found.
    held: Vec<CopyJob>,
    /// Jobs queued or being executed.
    pending: usize,
    active: usize,
    closed: bool,
}

pub struct CopyQueue {
    state: Mutex<State>,
    cv: Condvar,
    max: usize,
    retries: u32,
    pub reuse: Reuse,
}

impl CopyQueue {
    pub fn new(max: usize, retries: u32) -> Arc<CopyQueue> {
        Arc::new(CopyQueue {
            state: Mutex::new(State { queue: VecDeque::new(), held: Vec::new(), pending: 0, active: 0, closed: false }),
            cv: Condvar::new(),
            max: max.max(1),
            retries: retries.max(1),
            reuse: Reuse::default(),
        })
    }

    pub fn enqueue(&self, job: CopyJob) {
        let mut s = self.state.lock().unwrap();
        s.pending += 1;
        if job.held {
            s.held.push(job);
            return;
        }
        s.queue.push_back(job);
        self.cv.notify_one();
    }

    /// The destination peers of held copies, once each.
    pub fn held_peers(&self) -> Vec<PeerRef> {
        let s = self.state.lock().unwrap();
        let mut out: Vec<PeerRef> = Vec::new();
        for j in &s.held {
            if !out.iter().any(|p| p.index == j.dst.index) {
                out.push(Arc::clone(&j.dst));
            }
        }
        out
    }

    /// The walk is over: let the held copies run.
    pub fn release_held(&self) {
        let mut s = self.state.lock().unwrap();
        let held = std::mem::take(&mut s.held);
        s.queue.extend(held);
        self.cv.notify_all();
    }

    /// Start the worker threads (one per copy slot).
    pub fn start_workers(self: &Arc<Self>) -> Vec<JoinHandle<()>> {
        (0..self.max)
            .map(|_| {
                let q = Arc::clone(self);
                std::thread::spawn(move || q.worker())
            })
            .collect()
    }

    /// Mark that no more jobs will be enqueued and wait for all copies.
    pub fn close_and_wait(&self, workers: Vec<JoinHandle<()>>) {
        {
            let mut s = self.state.lock().unwrap();
            s.closed = true;
            self.cv.notify_all();
        }
        for w in workers {
            let _ = w.join();
        }
    }

    fn worker(&self) {
        loop {
            let mut job = {
                let mut s = self.state.lock().unwrap();
                loop {
                    if let Some(j) = s.queue.pop_front() {
                        s.active += 1;
                        output::trace(&format!("copy-slots active={}/{}", s.active, self.max));
                        break j;
                    }
                    if s.closed {
                        return;
                    }
                    s = self.cv.wait(s).unwrap();
                }
            };
            job.tries += 1;
            let (outcome, origin) = transfer(&job, &self.reuse);
            let mut s = self.state.lock().unwrap();
            s.active -= 1;
            output::trace(&format!("copy-slots active={}/{}", s.active, self.max));
            let finished: Option<bool> = match outcome {
                Outcome::Done => Some(true),
                Outcome::Retry => {
                    if job.tries < self.retries {
                        None
                    } else {
                        output::error(&format!("giving up on {} to {} after {} tries", job.path, job.dst.url, job.tries));
                        Some(false)
                    }
                }
                Outcome::Skip => Some(false),
            };
            match finished {
                Some(ok) => {
                    s.pending -= 1;
                    drop(s);
                    if !ok {
                        note_failure();
                    }
                    job.dst_dir.copy_finished(crate::util::basename(&job.path), ok, origin);
                }
                None => {
                    s.queue.push_back(job);
                    drop(s);
                }
            }
            self.cv.notify_all();
        }
    }
}

enum Outcome {
    Done,
    /// Failed before the existing destination was moved; may be retried.
    Retry,
    /// Failed in a way that must not be retried this run.
    Skip,
}

const BUF: usize = 1 << 20;

fn fail(job: &CopyJob, phase: &str, e: &crate::transport::TransportError) {
    output::error(&format!("transfer failed for {} to {}: {}: {}", job.path, job.dst.url, phase, kind_name(e)));
}

/// Stream the source to `dst_path` on the destination transport.
fn pump(src: &dyn Transport, src_path: &str, dst: &dyn Transport, dst_path: &str) -> std::result::Result<(), (&'static str, crate::transport::TransportError)> {
    let mut reader = src.open_read(src_path).map_err(|e| ("read_source", e))?;
    let mut writer = dst.open_write(dst_path).map_err(|e| ("write_swap_new", e))?;
    let mut buf = vec![0u8; BUF];
    loop {
        let n = reader.read(&mut buf).map_err(|e| ("read_source", e))?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).map_err(|e| ("write_swap_new", e))?;
    }
    writer.close().map_err(|e| ("write_swap_new", e))?;
    Ok(())
}

fn cleanup_staging(t: &dyn Transport, swap: &str) {
    let _ = fsops::remove_tree(t, swap);
}

/// Read `len` bytes at `offset`, or fewer at end of file.
fn read_at(r: &mut dyn crate::transport::ReadHandle, offset: u64, len: usize) -> Result<Vec<u8>> {
    r.seek(offset)?;
    let mut buf = vec![0u8; len];
    let mut filled = 0;
    while filled < len {
        let n = r.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Ok(buf)
}

const SAMPLE: usize = 64 * 1024;

/// Whether the candidate holds the same bytes as the source at the start, the
/// middle and the end (specs/sync.md, "Reusing A Displaced File").
fn same_samples(job: &CopyJob, candidate: &str) -> bool {
    let check = || -> Result<bool> {
        let mut a = job.src.transport.open_read(&job.path)?;
        let mut b = job.dst.transport.open_read(candidate)?;
        let size = job.byte_size.max(0) as u64;
        let last = size.saturating_sub(SAMPLE as u64);
        for off in [0, last / 2, last] {
            let x = read_at(a.as_mut(), off, SAMPLE)?;
            if x.is_empty() || x != read_at(b.as_mut(), off, SAMPLE)? {
                return Ok(false);
            }
        }
        Ok(true)
    };
    check().unwrap_or(false)
}

/// Copy one file. Returns the outcome and, for a reuse, the BAK path the file
/// was moved from.
fn transfer(job: &CopyJob, reuse: &Reuse) -> (Outcome, Option<String>) {
    let src: &dyn Transport = job.src.transport.as_ref();
    let dst: &dyn Transport = job.dst.transport.as_ref();
    let path = job.path.as_str();

    let parent = parent_path(path);
    let base = basename(path);
    let swap = swap_dir_for(path);
    let new = join(&swap, "new");
    let old = join(&swap, "old");

    // Any leftover SWAP state for this basename must be resolved first.
    if let Err(e) = fsops::recover_one_swap(dst, parent, base) {
        fail(job, "write_swap_new", &e);
        return (Outcome::Retry, None);
    }

    // A held copy looks for a displaced file to reuse, once, and says which
    // it is doing (the walk printed nothing for it).
    let mut origin: Option<String> = None;
    if job.held && job.tries == 1 {
        origin = reuse.take(job.dst.index, job.byte_size, job.mod_time, |c| same_samples(job, c));
        match &origin {
            Some(_) => output::info(&format!("M{} {path}", job.dst.tag())),
            None => output::info(&format!("{}C{} {path}", job.src.tag(), job.dst.tag())),
        }
    }

    // 1. Transfer to SWAP new. A reused file skips this: it goes straight
    //    from BAK to the final path in step 3, so an interrupted run can
    //    never mistake it for a cut-short transfer and delete it.
    if origin.is_none() {
        if let Err((phase, e)) = pump(src, path, dst, &new) {
            fail(job, phase, &e);
            cleanup_staging(dst, &swap);
            return (Outcome::Retry, None);
        }
    }

    // 2. Move any existing destination aside.
    let existing = match dst.stat(path) {
        Ok(_) => true,
        Err(e) if e.is_not_found() => false,
        Err(e) => {
            fail(job, "move_existing_to_swap_old", &e);
            cleanup_staging(dst, &swap);
            return (Outcome::Retry, None);
        }
    };
    if existing {
        if let Err(e) = dst.create_dir(&swap).and_then(|_| dst.rename(path, &old)) {
            fail(job, "move_existing_to_swap_old", &e);
            cleanup_staging(dst, &swap);
            return (Outcome::Skip, None);
        }
    }

    // 3. Swap in.
    let staged = origin.as_deref().unwrap_or(&new);
    if let Err(e) = dst.rename(staged, path) {
        fail(job, "rename_final", &e);
        if origin.is_some() {
            // The reused file is still in BAK: put the destination back and
            // try again as a plain transfer.
            if existing {
                let _ = dst.rename(&old, path);
            }
            cleanup_staging(dst, &swap);
            return (Outcome::Retry, None);
        }
        return (Outcome::Skip, None);
    }

    // 4. Set the winning mod_time.
    if let Err(e) = dst.set_mod_time(path, micros_to_system(job.mod_time)) {
        fail(job, "set_mod_time", &e);
    }

    // 5. Archive old.
    if existing {
        let bak = join(&meta_dir(parent), &format!("BAK/{}", now_string()));
        let archived: Result<()> = dst.create_dir(&bak).and_then(|_| dst.rename(&old, &join(&bak, base)));
        if let Err(e) = archived {
            fail(job, "archive_old", &e);
            return (Outcome::Done, origin);
        }
    }

    // 6. Clean up staging.
    if let Err(e) = fsops::delete_swap_dir(dst, &swap) {
        if !e.is_not_found() {
            fail(job, "cleanup", &e);
        }
    }
    (Outcome::Done, origin)
}
