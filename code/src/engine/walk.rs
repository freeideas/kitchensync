//! The combined-tree walk: list, decide, act, recurse.
//! See specs/multi-tree-sync.md.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Condvar, Mutex};

use crate::config::Config;
use crate::manifest::Line;
use crate::output;
use crate::transport::{join, Entry, Transport};
use crate::util::system_to_micros;

use super::copy::{CopyJob, CopyQueue};
use super::fsops::{self, PeerRef};
use super::peer::{self, DirState};

const TOL: i64 = 5_000_000; // 5 seconds in microseconds

pub struct Walker {
    pub cfg: Config,
    pub queue: Arc<CopyQueue>,
    pub ignore: crate::ignore::IgnoreSet,
    pub prefetch: Prefetch,
}

/// Directories to list ahead of the walk, and the listings that are ready.
///
/// A listing is mostly waiting on round trips, so a few threads list
/// directories the walk will need soon while it works on the current one.
/// The walk pushes a directory's subdirectories here once its entries are
/// settled, and takes each listing back when it recurses. The stack is
/// last-in-first-out so that the workers stay just ahead of the depth-first
/// walk, and the number of ready-but-unconsumed listings is capped so they
/// never run far ahead (a listing goes stale if it waits too long). Nothing
/// but listing and manifest reading runs ahead; decisions, copies and
/// displacements happen in the walk, in the usual order.
pub struct Prefetch {
    inner: Mutex<PrefetchState>,
    changed: Condvar,
}

/// A directory's position in the walk: its path components in the walk's
/// sort order (case-insensitive, original case as tie-breaker). Pre-order
/// traversal visits directories in ascending key order, so "the next
/// directory the walk needs" is simply the smallest key not yet visited.
type WalkKey = Vec<(String, String)>;

fn walk_key(rel: &str) -> WalkKey {
    rel.split('/').filter(|c| !c.is_empty()).map(|c| (c.to_lowercase(), c.to_string())).collect()
}

#[derive(Default)]
struct PrefetchState {
    /// Directories not yet started, in walk order.
    todo: BTreeMap<WalkKey, (String, Vec<PeerRef>)>,
    in_progress: HashSet<String>,
    /// Listings by directory, with the indices of the peers they were taken
    /// on, in order. The walk uses one only when it wants exactly those peers.
    ready: BTreeMap<WalkKey, (Vec<usize>, Vec<Option<Listed>>)>,
    /// Where the walk is; everything before it is no longer needed.
    position: WalkKey,
    stopped: bool,
}

/// Listing threads, and the cap on ready listings waiting for the walk.
pub const PREFETCH_THREADS: usize = 8;
const PREFETCH_READY_CAP: usize = 64;

impl Prefetch {
    pub fn new() -> Prefetch {
        Prefetch { inner: Mutex::new(PrefetchState::default()), changed: Condvar::new() }
    }

    /// Queue directories for listing, unless already queued, listed or passed.
    fn push(&self, dirs: Vec<(String, Vec<PeerRef>)>) {
        let mut st = self.inner.lock().unwrap();
        for (rel, peers) in dirs {
            let key = walk_key(&rel);
            if key < st.position || st.in_progress.contains(&rel) || st.ready.contains_key(&key) {
                continue;
            }
            st.todo.entry(key).or_insert((rel, peers));
        }
        self.changed.notify_all();
    }

    /// Stop the workers once the walk is over.
    pub fn stop(&self) {
        self.inner.lock().unwrap().stopped = true;
        self.changed.notify_all();
    }
}

impl Default for Prefetch {
    fn default() -> Self {
        Self::new()
    }
}

/// One peer's view of one entry name at the current level.
struct View {
    state: Arc<DirState>,
    live: Option<Entry>,
    line: Option<Line>,
}

impl View {
    fn peer(&self) -> &PeerRef {
        &self.state.peer
    }
    fn live_file(&self) -> Option<&Entry> {
        self.live.as_ref().filter(|e| !e.is_dir)
    }
    fn live_dir(&self) -> Option<&Entry> {
        self.live.as_ref().filter(|e| e.is_dir)
    }
    fn mt(&self) -> Option<i64> {
        self.live.as_ref().map(|e| system_to_micros(e.mod_time))
    }
}

fn within(a: i64, b: i64) -> bool {
    (a - b).abs() <= TOL
}

/// Listing plus manifest for one peer at one directory.
pub struct Listed {
    state: Arc<DirState>,
    entries: Vec<Entry>,
}

impl Walker {
    fn excluded(&self, rel: &str, is_dir: bool) -> bool {
        self.ignore.is_excluded(rel, is_dir)
    }

    fn list_peer(&self, p: &PeerRef, dir: &str) -> Option<Listed> {
        let t: &dyn Transport = p.transport.as_ref();
        if !self.cfg.dry_run {
            if let Err(e) = peer::recover_manifest(t, dir).and_then(|_| fsops::recover_swaps(t, dir)) {
                output::error(&format!("recovery failed for {} at {}: {}", p.url, dir, e));
                return None;
            }
        }
        // The listing and the manifest read are independent round trips:
        // issue them together.
        let t0 = std::time::Instant::now();
        let (entries, text) = std::thread::scope(|s| {
            let manifest = s.spawn(|| peer::read_manifest(t, dir));
            let entries = fsops::list_with_retries(t, dir, self.cfg.retries_list);
            (entries, manifest.join().unwrap_or_else(|_| Err(crate::transport::TransportError::io("manifest read thread failed".to_string()))))
        });
        let t1 = std::time::Instant::now();
        let entries = match entries {
            Ok(v) => v,
            // In a dry run a directory that would have been created does not
            // exist yet; treat it as empty so the copies into it are still planned.
            // Only when the directory itself is absent: a "not found" raised by
            // something inside an existing directory is a real listing failure.
            Err(e) if self.cfg.dry_run && e.is_not_found() && t.stat(dir).is_err() => {
                return Some(Listed { state: DirState::new(Arc::clone(p), dir, None, true, self.cfg.keep_del_days), entries: Vec::new() });
            }
            Err(e) => {
                output::error(&format!("listing failed for {} at {}, excluding from this subtree: {}", p.url, dir, e));
                return None;
            }
        };
        let text = match text {
            Ok(v) => v,
            Err(e) => {
                output::error(&format!("manifest read failed for {} at {}, excluding from this subtree: {}", p.url, dir, e));
                return None;
            }
        };
        if output::level() >= output::Verbosity::Trace {
            output::trace(&format!(
                "listed {} on {}: {} entries, manifest {} bytes, in {} ms",
                if dir.is_empty() { "." } else { dir },
                p.tag(),
                entries.len(),
                text.as_ref().map_or(0, |t| t.len()),
                (t1 - t0).as_millis()
            ));
        }
        Some(Listed { state: DirState::new(Arc::clone(p), dir, text, self.cfg.dry_run, self.cfg.keep_del_days), entries })
    }

    /// Sync one directory level across `peers`, then recurse. Returns whether
    /// any entry remains live in the directory after this run's decisions.
    /// Phase 0/1 for one directory: recover and list every peer in parallel.
    fn list_all(&self, peers: &[PeerRef], dir: &str) -> Vec<Option<Listed>> {
        std::thread::scope(|s| {
            let handles: Vec<_> = peers.iter().map(|p| s.spawn(move || self.list_peer(p, dir))).collect();
            handles.into_iter().map(|h| h.join().unwrap_or(None)).collect()
        })
    }

    /// Body of one prefetch thread: list the most recently queued directory
    /// until the walk stops. Waits while enough listings are already ready.
    pub fn prefetch_worker(&self) {
        loop {
            let (dir, peers) = {
                let mut st = self.prefetch.inner.lock().unwrap();
                loop {
                    if st.stopped {
                        return;
                    }
                    if !st.todo.is_empty() && st.ready.len() < PREFETCH_READY_CAP {
                        break;
                    }
                    st = self.prefetch.changed.wait(st).unwrap();
                }
                let item = st.todo.pop_first().unwrap().1;
                st.in_progress.insert(item.0.clone());
                item
            };
            let listings = self.list_all(&peers, &dir);
            // Queue this directory's subdirectories right away, on the peers
            // that list them, so a deep chain of single subdirectories is
            // fetched ahead too. Listing is read-only, so running ahead of the
            // walk's decisions costs nothing but a wasted listing when the
            // walk displaces a directory instead of entering it.
            let children = self.subdirectories(&dir, &listings);
            let indices: Vec<usize> = peers.iter().map(|p| p.index).collect();
            let mut st = self.prefetch.inner.lock().unwrap();
            st.in_progress.remove(&dir);
            st.ready.insert(walk_key(&dir), (indices, listings));
            drop(st);
            self.prefetch.push(children);
        }
    }

    /// The subdirectories a listing shows, in walk order, each with the peers
    /// that list it as a directory.
    fn subdirectories(&self, dir: &str, listings: &[Option<Listed>]) -> Vec<(String, Vec<PeerRef>)> {
        let mut names: BTreeSet<&str> = BTreeSet::new();
        for l in listings.iter().flatten() {
            for e in l.entries.iter().filter(|e| e.is_dir) {
                if e.name != ".kitchensync" && e.name != ".git" {
                    names.insert(&e.name);
                }
            }
        }
        let mut ordered: Vec<&str> = names.into_iter().collect();
        ordered.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
        ordered
            .into_iter()
            .map(|name| (join(dir, name), name))
            .filter(|(rel, _)| !self.excluded(rel, true))
            .map(|(rel, name)| {
                let peers = listings
                    .iter()
                    .flatten()
                    .filter(|l| l.entries.iter().any(|e| e.is_dir && e.name == name))
                    .map(|l| Arc::clone(&l.state.peer))
                    .collect();
                (rel, peers)
            })
            .collect()
    }

    /// The listings for `dir`: ready ones are taken, in-progress ones waited
    /// for, and the rest listed right here.
    fn take_listings(&self, peers: &[PeerRef], dir: &str) -> Vec<Option<Listed>> {
        let wanted: Vec<usize> = peers.iter().map(|p| p.index).collect();
        let key = walk_key(dir);
        {
            let mut st = self.prefetch.inner.lock().unwrap();
            // The walk has moved here: whatever was queued or fetched for
            // directories before this one (skipped, displaced, or excluded)
            // will never be asked for.
            st.position = key.clone();
            st.todo = st.todo.split_off(&key);
            st.ready = st.ready.split_off(&key);
            loop {
                if let Some((indices, l)) = st.ready.remove(&key) {
                    self.prefetch.changed.notify_all();
                    if indices == wanted {
                        return l;
                    }
                    // Fetched on a different peer set (a directory the walk
                    // created or dropped a peer for): list it again below.
                    break;
                }
                if !st.in_progress.contains(dir) {
                    st.todo.remove(&key);
                    self.prefetch.changed.notify_all();
                    break;
                }
                st = self.prefetch.changed.wait(st).unwrap();
            }
        }
        self.list_all(peers, dir)
    }

    /// A long silence worries the person watching: say where the walk is.
    fn heartbeat(dir: &str) {
        if output::quiet_for_a_while() {
            output::info(&format!("S {}", if dir.is_empty() { "." } else { dir }));
        }
    }

    pub fn sync_directory(&self, peers: &[PeerRef], dir: &str) -> bool {
        Self::heartbeat(dir);
        // Phase 0/1: the listings, fetched ahead where the prefetch got to
        // them first (see `Prefetch`).
        let listings = self.take_listings(peers, dir);

        // Phase 1b: drop failed peers.
        if peers.iter().zip(&listings).any(|(p, l)| p.is_canon() && l.is_none()) {
            return true;
        }
        let active: Vec<Listed> = listings.into_iter().flatten().collect();
        if !active.iter().any(|l| l.state.peer.contributes()) {
            return true;
        }

        // Phase 2: union of names, minus excludes. A name counts as a
        // directory for pattern purposes if any peer lists it as one.
        let mut names: BTreeSet<String> = BTreeSet::new();
        let mut kept_excluded = false;
        for l in &active {
            for e in &l.entries {
                if e.name == ".kitchensync" || e.name == ".git" {
                    continue;
                }
                let is_dir = e.is_dir || active.iter().any(|o| o.entries.iter().any(|x| x.name == e.name && x.is_dir));
                if self.excluded(&join(dir, &e.name), is_dir) {
                    kept_excluded = true;
                    continue;
                }
                names.insert(e.name.clone());
            }
        }
        let mut ordered: Vec<String> = names.into_iter().collect();
        ordered.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));

        // Phase 3: decide and act on every entry here. Subdirectories are
        // prepared (displaced, created) in this pass but recursed into only
        // after every entry of this directory is settled, so that changes in
        // a shallow directory are found and acted on before deep ones.
        let mut kept = kept_excluded;
        let mut pending: Vec<PendingDir> = Vec::new();
        for name in ordered {
            let rel = join(dir, &name);
            let views: Vec<View> = active
                .iter()
                .map(|l| View {
                    state: Arc::clone(&l.state),
                    live: l.entries.iter().find(|e| e.name == name).cloned(),
                    line: if l.state.peer.contributes() { l.state.get(&name) } else { None },
                })
                .collect();
            match self.decide(&views) {
                Decision::Directory { conflict } => {
                    if let Some(p) = self.prepare_directory(views, rel, name, conflict) {
                        pending.push(p);
                    }
                }
                Decision::File { src, mod_time, byte_size } => {
                    self.apply_file(&views, &rel, &name, src, mod_time, byte_size);
                    kept = true;
                }
                Decision::Delete => self.apply_delete(&views, &rel, &name),
            }
        }

        for l in &active {
            l.state.finish_deciding();
        }

        // Phase 3b: recurse into the subdirectories prepared above, in order,
        // after handing them to the prefetch so their listings are fetched
        // while the walk is busy.
        self.prefetch.push(pending.iter().map(|p| (p.rel.clone(), p.recurse.clone())).collect());
        for p in &pending {
            if self.recurse_directory(p) {
                kept = true;
            }
        }

        // BAK cleanup piggybacks on the traversal.
        if !self.cfg.dry_run {
            for l in &active {
                fsops::cleanup_bak(&l.state.peer, dir, self.cfg.keep_bak_days);
            }
        }
        kept
    }

    fn decide(&self, views: &[View]) -> Decision {
        let contributing: Vec<&View> = views.iter().filter(|v| v.peer().contributes()).collect();

        // Canon wins unconditionally.
        if let Some(c) = contributing.iter().find(|v| v.peer().is_canon()) {
            return match &c.live {
                Some(e) if e.is_dir => Decision::Directory { conflict: false },
                Some(e) => Decision::File { src: c.peer().index, mod_time: system_to_micros(e.mod_time), byte_size: e.byte_size },
                None => Decision::Delete,
            };
        }

        let any_file = contributing.iter().any(|v| v.live_file().is_some());
        let any_dir = contributing.iter().any(|v| v.live_dir().is_some());
        if any_file {
            // File wins any type conflict among contributing peers.
            return self.decide_file(&contributing);
        }
        if any_dir {
            return self.decide_directory(&contributing);
        }
        // Only subordinate peers have it (or nobody): not part of the group's view.
        Decision::Delete
    }

    /// Rules 1-7 over contributing peers' file entries.
    fn decide_file(&self, contributing: &[&View]) -> Decision {
        let live: Vec<&View> = contributing.iter().copied().filter(|v| v.live_file().is_some()).collect();
        let max_mt = live.iter().filter_map(|v| v.mt()).max().unwrap();

        // Deletion votes: tombstones, or unconfirmed absences whose last_seen
        // exceeds the newest live mod_time.
        let mut estimate: Option<i64> = None;
        for v in contributing.iter().filter(|v| v.live.is_none()) {
            let Some(line) = &v.line else { continue };
            let est = match line.deleted_time {
                Some(d) => Some(d),
                None => line.last_seen.filter(|ls| *ls > max_mt + TOL),
            };
            if let Some(e) = est {
                estimate = Some(estimate.map_or(e, |cur: i64| cur.max(e)));
            }
        }
        if let Some(est) = estimate {
            if est > max_mt + TOL {
                return Decision::Delete;
            }
        }

        // Existence wins: newest mod_time, larger size breaks ties.
        let mut winners: Vec<&View> = live.iter().copied().filter(|v| within(v.mt().unwrap(), max_mt)).collect();
        winners.sort_by_key(|v| std::cmp::Reverse(v.live_file().unwrap().byte_size));
        let w = winners[0];
        let e = w.live_file().unwrap();
        Decision::File { src: w.peer().index, mod_time: system_to_micros(e.mod_time), byte_size: e.byte_size }
    }

    /// Directory Decisions: a deletion vote against a live directory means the
    /// directory survives this run and its contents are decided entry by entry.
    fn decide_directory(&self, contributing: &[&View]) -> Decision {
        let conflict = contributing.iter().any(|v| v.live.is_none() && v.line.is_some());
        Decision::Directory { conflict }
    }

    /// Print one `X<peers> <relpath>` line for the peers collected so far.
    fn print_x(rel: &str, tags: &mut String) {
        if !tags.is_empty() {
            output::info(&format!("X{tags} {rel}"));
            tags.clear();
        }
    }

    fn displace_view(&self, v: &View, rel: &str, name: &str, tags: &mut String) -> bool {
        tags.push(v.peer().tag());
        if fsops::displace(v.peer(), rel, self.cfg.dry_run) {
            v.state.confirm_absent(name);
            true
        } else {
            false
        }
    }

    /// Make the directory exist on every peer that should have it (displacing
    /// a wrong-typed entry first) and return what recursion into it needs, or
    /// None when no peer keeps it.
    fn prepare_directory(&self, views: Vec<View>, rel: String, name: String, conflict: bool) -> Option<PendingDir> {
        let mut tags = String::new();
        let mut recurse: Vec<PeerRef> = Vec::new();
        for v in &views {
            match &v.live {
                Some(e) if e.is_dir => {
                    v.state.confirm_present(&name, true, system_to_micros(e.mod_time), -1);
                    recurse.push(Arc::clone(v.peer()));
                }
                other => {
                    if other.is_some() && !self.displace_view(v, &rel, &name, &mut tags) {
                        continue;
                    }
                    if !self.cfg.dry_run {
                        if let Err(e) = v.peer().transport.create_dir(&rel) {
                            output::error(&format!("create directory failed for {} on {}: {}", rel, v.peer().url, e));
                            peer::note_failure();
                            continue;
                        }
                    }
                    v.state.created_dir(&name);
                    recurse.push(Arc::clone(v.peer()));
                }
            }
        }
        Self::print_x(&rel, &mut tags);
        if recurse.is_empty() {
            return None;
        }
        Some(PendingDir { views, rel, name, conflict, recurse })
    }

    /// Returns whether the directory still exists after this run.
    fn recurse_directory(&self, p: &PendingDir) -> bool {
        let kept = self.sync_directory(&p.recurse, &p.rel);
        if p.conflict && !kept {
            // Everything inside was older than the deletion: the directory goes too.
            let mut tags = String::new();
            for v in p.views.iter().filter(|v| p.recurse.iter().any(|r| r.index == v.peer().index)) {
                self.displace_view(v, &p.rel, &p.name, &mut tags);
            }
            Self::print_x(&p.rel, &mut tags);
            return false;
        }
        true
    }

    fn apply_file(&self, views: &[View], rel: &str, name: &str, src: usize, mod_time: i64, byte_size: i64) {
        let src_peer = views.iter().find(|v| v.peer().index == src).map(|v| Arc::clone(v.peer())).unwrap();
        let mut x_tags = String::new();
        let mut dsts: Vec<&View> = Vec::new();
        for v in views {
            match &v.live {
                Some(e) if !e.is_dir && within(system_to_micros(e.mod_time), mod_time) && e.byte_size == byte_size => {
                    v.state.confirm_present(name, false, system_to_micros(e.mod_time), e.byte_size);
                    continue;
                }
                Some(e) if e.is_dir => {
                    // Type conflict: the directory must go before the file can land.
                    if !self.displace_view(v, rel, name, &mut x_tags) {
                        continue;
                    }
                }
                _ => {}
            }
            dsts.push(v);
        }
        Self::print_x(rel, &mut x_tags);
        if dsts.is_empty() {
            return;
        }
        let c_tags: String = dsts.iter().map(|v| v.peer().tag()).collect();
        output::info(&format!("{}C{c_tags} {rel}", src_peer.tag()));
        if self.cfg.dry_run {
            return;
        }
        for v in dsts {
            v.state.intend_push(name, mod_time, byte_size);
            self.queue.enqueue(CopyJob {
                src: Arc::clone(&src_peer),
                dst: Arc::clone(v.peer()),
                dst_dir: Arc::clone(&v.state),
                path: rel.to_string(),
                mod_time,
                tries: 0,
            });
        }
    }

    fn apply_delete(&self, views: &[View], rel: &str, name: &str) {
        let mut tags = String::new();
        for v in views {
            if v.live.is_some() {
                self.displace_view(v, rel, name, &mut tags);
            } else {
                v.state.confirm_absent(name);
            }
        }
        Self::print_x(rel, &mut tags);
    }
}

/// A subdirectory whose entry has been settled here and that still has to be
/// recursed into, on the peers that keep it.
struct PendingDir {
    views: Vec<View>,
    rel: String,
    name: String,
    conflict: bool,
    recurse: Vec<PeerRef>,
}

enum Decision {
    Directory { conflict: bool },
    File { src: usize, mod_time: i64, byte_size: i64 },
    Delete,
}
