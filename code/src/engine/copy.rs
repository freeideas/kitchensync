//! The file-copy queue and the SWAP-based transfer procedure, including
//! moving a file the destination already holds into place.
//! See specs/sync.md ("File Copy", "Rename Compatibility", "Moved Files") and
//! specs/concurrency.md ("Copy Concurrency", "Copy Queue Tries").

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use crate::output;
use crate::transport::{join, Entry, Result, Transport};
use crate::util::{basename, micros_to_system, parent_path, system_to_micros};

use super::fsops::{self, kind_name, meta_dir, swap_dir_for, PeerRef};
use super::moves::MOVE_MIN;
use super::peer::{in_bak, note_failure, DirState, Peer};

pub struct CopyJob {
    pub src: PeerRef,
    pub dst: PeerRef,
    /// The destination directory's record, told when we finish.
    pub dst_dir: Arc<DirState>,
    pub path: String,
    pub mod_time: i64,
    pub byte_size: i64,
    /// A candidate the walk reserved on the destination (see `moves`).
    pub reserved: Option<usize>,
    pub tries: u32,
}

struct State {
    queue: VecDeque<CopyJob>,
    active: usize,
    closed: bool,
}

pub struct CopyQueue {
    state: Mutex<State>,
    cv: Condvar,
    max: usize,
    retries: u32,
    /// Every peer in the run, for the checks a move needs.
    peers: Vec<PeerRef>,
}

impl CopyQueue {
    pub fn new(max: usize, retries: u32, peers: Vec<PeerRef>) -> Arc<CopyQueue> {
        Arc::new(CopyQueue { state: Mutex::new(State { queue: VecDeque::new(), active: 0, closed: false }), cv: Condvar::new(), max: max.max(1), retries: retries.max(1), peers })
    }

    pub fn peers(&self) -> &[PeerRef] {
        &self.peers
    }

    pub fn enqueue(&self, job: CopyJob) {
        self.state.lock().unwrap().queue.push_back(job);
        self.cv.notify_one();
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
            let outcome = transfer(&mut job, &self.peers);
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
                    drop(s);
                    if !ok {
                        note_failure();
                    }
                    job.dst_dir.copy_finished(basename(&job.path), ok);
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

/// Copies staging in each (peer, directory). When the last one there is done,
/// the directory's SWAP folder, and below the root its `.kitchensync`
/// folder, are removed if empty (specs/sync.md, "SWAP Directory").
static STAGING: Mutex<Option<HashMap<(usize, String), usize>>> = Mutex::new(None);

fn stage_start(peer: &Peer, dir: &str) {
    let mut g = STAGING.lock().unwrap();
    *g.get_or_insert_with(HashMap::new).entry((peer.index, dir.to_string())).or_default() += 1;
}

fn stage_end(peer: &Peer, dir: &str) {
    let mut g = STAGING.lock().unwrap();
    let map = g.get_or_insert_with(HashMap::new);
    let key = (peer.index, dir.to_string());
    let n = map.get_mut(&key).expect("staging count");
    *n -= 1;
    if *n > 0 {
        return;
    }
    map.remove(&key);
    // Still holding the lock: no other copy can start staging here meanwhile.
    let t = peer.t();
    let meta = meta_dir(dir);
    if fsops::delete_litter_dir(t, &join(&meta, "SWAP")).is_ok() && !dir.is_empty() {
        let _ = fsops::delete_litter_dir(t, &meta);
    }
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
const TOL: i64 = 5_000_000;

/// Whether the candidate holds the same bytes as the source at the start, the
/// middle and the end.
fn same_samples(src: &Peer, path: &str, dst: &Peer, candidate: &str, size: i64) -> bool {
    let check = || -> Result<bool> {
        let mut a = src.t().open_read(path)?;
        let mut b = dst.t().open_read(candidate)?;
        let last = (size.max(0) as u64).saturating_sub(SAMPLE as u64);
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

/// Rules 2 (the part that needs the peers) to 4 of "Moved Files" for one
/// candidate. A dry run skips rule 4: it reads no file content.
pub fn candidate_fits(peers: &[PeerRef], src: &Peer, path: &str, dst: &Peer, candidate: &str, size: i64, mod_time: i64, read_content: bool) -> bool {
    if !in_bak(candidate) {
        for p in peers.iter().filter(|p| p.contributes() && p.index != dst.index) {
            match p.t().stat(candidate) {
                Err(e) if e.is_not_found() => {}
                _ => return false,
            }
        }
    }
    match dst.t().stat(candidate) {
        Ok(e) if !e.is_dir && e.byte_size == size && (system_to_micros(e.mod_time) - mod_time).abs() <= TOL => {}
        _ => return false,
    }
    !read_content || same_samples(src, path, dst, candidate, size)
}

/// Find a candidate to move into place for this copy, or None to transfer.
/// A reserved candidate that does not fit is displaced here when the walk
/// wanted it gone meanwhile.
fn find_move(job: &CopyJob, peers: &[PeerRef]) -> Option<usize> {
    let ix = &job.dst.moves;
    let fits = |id: usize| candidate_fits(peers, &job.src, &job.path, &job.dst, &ix.path(id), job.byte_size, job.mod_time, true);
    let mut tried = Vec::new();
    if let Some(id) = job.reserved {
        if fits(id) {
            return Some(id);
        }
        let at = ix.path(id);
        if ix.release(id) {
            let entry = Entry { name: basename(&at).to_string(), is_dir: false, mod_time: micros_to_system(job.mod_time), byte_size: job.byte_size };
            if fsops::displace(&job.dst, &at, Some(&entry), false) {
                output::info(&format!("X{} {at}", job.dst.tag()));
            }
        }
        tried.push(id);
    }
    // Other candidates already in BAK need no word from the walk.
    loop {
        let id = ix.reserve(job.byte_size, job.mod_time, &job.path, &tried, |_| false)?;
        if fits(id) {
            return Some(id);
        }
        ix.release(id);
        tried.push(id);
    }
}

/// Copy one file, or move a file the destination holds into place.
fn transfer(job: &mut CopyJob, peers: &[PeerRef]) -> Outcome {
    let src: &dyn Transport = job.src.t();
    let dst: &dyn Transport = job.dst.t();
    let path = job.path.clone();
    let parent = parent_path(&path).to_string();
    let base = basename(&path);
    let swap = swap_dir_for(&path);
    let new = join(&swap, "new");
    let old = join(&swap, "old");

    // A try after a failure first resolves what the earlier try left.
    if job.tries > 1 {
        if let Err(e) = fsops::recover_one_swap(&job.dst, &parent, base) {
            fail(job, "write_swap_new", &e);
            return Outcome::Retry;
        }
    }

    // A large file looks for itself elsewhere on the destination, once, and
    // says which it is doing (the walk printed nothing for it).
    let mut moving: Option<usize> = None;
    if job.byte_size >= MOVE_MIN && job.tries == 1 {
        moving = find_move(job, peers);
        job.reserved = None;
        match moving {
            Some(_) => output::info(&format!("M{} {path}", job.dst.tag())),
            None => output::info(&format!("{}C{} {path}", job.src.tag(), job.dst.tag())),
        }
    }

    stage_start(&job.dst, &parent);
    let outcome = swap_in(job, src, dst, &path, &swap, &new, &old, moving);
    stage_end(&job.dst, &parent);
    outcome
}

#[allow(clippy::too_many_arguments)]
fn swap_in(job: &CopyJob, src: &dyn Transport, dst: &dyn Transport, path: &str, swap: &str, new: &str, old: &str, moving: Option<usize>) -> Outcome {
    let cleanup = || {
        let _ = fsops::remove_tree(dst, swap);
    };

    // 1. Transfer to SWAP new. A moved file skips this: it goes straight to
    //    the final path in step 3, so an interrupted run can never mistake
    //    it for a cut-short transfer and delete it.
    if moving.is_none() {
        if let Err((phase, e)) = pump(src, path, dst, new) {
            fail(job, phase, &e);
            cleanup();
            return Outcome::Retry;
        }
    }

    // 2. Move any existing destination aside.
    let existing = match dst.stat(path) {
        Ok(_) => true,
        Err(e) if e.is_not_found() => false,
        Err(e) => {
            fail(job, "move_existing_to_swap_old", &e);
            cleanup();
            return Outcome::Retry;
        }
    };
    if existing {
        if let Err(e) = dst.create_dir(swap).and_then(|_| dst.rename(path, old)) {
            fail(job, "move_existing_to_swap_old", &e);
            cleanup();
            return Outcome::Skip;
        }
    }

    // 3. Swap in.
    let mut moved_from: Option<String> = None;
    match moving {
        Some(id) => {
            let ix = &job.dst.moves;
            let _fs = ix.lock_fs();
            let from = ix.path(id);
            match dst.rename(&from, path) {
                Ok(()) => {
                    ix.used(id);
                    moved_from = Some(from);
                }
                Err(e) => {
                    drop(_fs);
                    fail(job, "rename_final", &e);
                    ix.release(id);
                    // The candidate is where it was: put the destination
                    // back and try again as a plain transfer.
                    if existing {
                        let _ = dst.rename(old, path);
                    }
                    cleanup();
                    return Outcome::Retry;
                }
            }
        }
        None => {
            if let Err(e) = dst.rename(new, path) {
                fail(job, "rename_final", &e);
                return Outcome::Skip;
            }
        }
    }

    // 4. Set the winning mod_time.
    if let Err(e) = dst.set_mod_time(path, micros_to_system(job.mod_time)) {
        fail(job, "set_mod_time", &e);
    }

    // 5. Archive old, then record the new content (old first, so a rollback
    //    undoes the new content before it puts the old back).
    if existing {
        if let Err(e) = fsops::archive(&job.dst, old, path, None) {
            fail(job, "archive_old", &e);
        }
    }
    match &moved_from {
        Some(from) => job.dst.journal('M', path, Some(from), Some(job.byte_size), Some(job.mod_time)),
        None => job.dst.journal('C', path, None, Some(job.byte_size), Some(job.mod_time)),
    }

    // 6. Clean up staging.
    if let Err(e) = fsops::delete_litter_dir(dst, swap) {
        if !e.is_not_found() {
            fail(job, "cleanup", &e);
        }
    }
    Outcome::Done
}
