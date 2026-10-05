//! A reachable peer: its history (the state file held in memory), its
//! journal, its BAK, and the per-directory record kept while a directory is
//! being decided. See specs/state.md.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::Role;
use crate::output;
use crate::state::{self, DirLines, JLine, Line, Tree};
use crate::transport::{join, Result, Transport, WriteHandle};
use crate::util::{now_micros, now_string};

use super::moves::Index;

pub const META: &str = ".kitchensync";

/// How often a long run saves what it has learned so far.
pub const CHECKPOINT: Duration = Duration::from_secs(300);

pub struct Peer {
    pub index: usize,
    pub role: Role,
    /// Normalized URL of the winning connection (for diagnostics).
    pub url: String,
    pub transport: Arc<dyn Transport>,
    /// Whether the sync root had history at startup.
    pub had_history: bool,
    /// The run's start timestamp: names this run's BAK folder and journal.
    pub run: String,
    pub dry_run: bool,
    pub keep_del_days: u64,
    pub history: History,
    /// Files this peer holds or held, for finding moved files.
    pub moves: Index,
    journal: Mutex<Option<Box<dyn WriteHandle>>>,
    /// Directories this run has already made (BAK parents), to skip the calls.
    made: Mutex<HashSet<String>>,
    /// Where the state, journal, BAK and run log live, relative to the sync
    /// root: "" for the root itself, or `..`, `../..` and so on when the
    /// root has no state and takes it from an ancestor (specs/state.md,
    /// "Syncing part of a tree").
    pub up: String,
    /// The sync root's path relative to that ancestor ("" for the root).
    /// The state, journal and BAK use paths relative to the ancestor.
    pub prefix: String,
    /// Nested sync roots this run absorbed; their own state is deleted once
    /// this peer's state has been written (specs/state.md, "Nested sync roots").
    pub absorbed: Mutex<Vec<String>>,
}

impl Peer {
    pub fn new(index: usize, role: Role, url: String, transport: Arc<dyn Transport>, had_history: bool, run: String, dry_run: bool, keep_del_days: u64, history: History) -> Peer {
        Peer { index, role, url, transport, had_history, run, dry_run, keep_del_days, history, moves: Index::default(), journal: Mutex::new(None), made: Mutex::new(HashSet::new()), up: String::new(), prefix: String::new(), absorbed: Mutex::new(Vec::new()) }
    }

    /// A path inside the `.kitchensync` folder that holds this peer's state.
    pub fn meta_at(&self, path: &str) -> String {
        join(&self.up, &meta(path))
    }

    /// A path as the state and journal record it: relative to the folder
    /// that holds the state. `path` is a sync-root path, or a path into that
    /// folder's BAK as `meta_at` builds it.
    pub fn to_anchor(&self, path: &str) -> String {
        if !self.up.is_empty() {
            if let Some(rest) = path.strip_prefix(&self.up).and_then(|r| r.strip_prefix('/')) {
                return rest.to_string();
            }
        }
        join(&self.prefix, path)
    }

    /// The reverse of `to_anchor`; None for a path outside this sync root.
    pub fn from_anchor(&self, path: &str) -> Option<String> {
        if path.starts_with(".kitchensync/") {
            return Some(join(&self.up, path));
        }
        if self.prefix.is_empty() {
            return Some(path.to_string());
        }
        if path == self.prefix {
            return Some(String::new());
        }
        path.strip_prefix(&self.prefix).and_then(|r| r.strip_prefix('/')).map(str::to_string)
    }

    pub fn is_canon(&self) -> bool {
        self.role == Role::Canon
    }
    pub fn contributes(&self) -> bool {
        self.role != Role::Subordinate
    }
    /// One-character peer label for progress lines: the peer's 1-based
    /// command-line position as a lower-case base-36 digit (1-9, then a-z).
    pub fn tag(&self) -> char {
        peer_tag(self.index)
    }
    pub fn t(&self) -> &dyn Transport {
        self.transport.as_ref()
    }

    /// Append one change to this run's journal, opening it on first use.
    pub fn journal(&self, op: char, path: &str, other: Option<&str>, byte_size: Option<i64>, mod_time: Option<i64>) {
        if self.dry_run {
            return;
        }
        let line = JLine { ts: now_micros(), op, path: self.to_anchor(path), other: other.map(|o| self.to_anchor(o)), byte_size, mod_time }.format();
        let mut g = self.journal.lock().unwrap();
        if g.is_none() {
            let opened = self.t().open_write(&self.meta_at(&format!("journal/{}.txt", self.run))).and_then(|mut w| {
                w.write_all(state::journal_header().as_bytes())?;
                Ok(w)
            });
            match opened {
                Ok(w) => *g = Some(w),
                Err(e) => {
                    output::error(&format!("journal write failed for {}: {}", self.url, e));
                    return;
                }
            }
        }
        if let Err(e) = g.as_mut().unwrap().write_all(line.as_bytes()) {
            output::error(&format!("journal write failed for {}: {}", self.url, e));
        }
    }

    pub fn close_journal(&self) {
        if let Some(w) = self.journal.lock().unwrap().take() {
            if let Err(e) = w.close() {
                output::error(&format!("journal write failed for {}: {}", self.url, e));
            }
        }
    }

    /// Create a directory (and parents) once per run.
    pub fn ensure_dir(&self, path: &str) -> Result<()> {
        if self.made.lock().unwrap().contains(path) {
            return Ok(());
        }
        self.t().create_dir(path)?;
        self.made.lock().unwrap().insert(path.to_string());
        Ok(())
    }

    /// Move `from` to BAK as the entry at `rel` (specs/state.md, "BAK") and
    /// return where it went. Writes no journal line; the caller does.
    pub fn move_to_bak(&self, from: &str, rel: &str) -> Result<String> {
        let to = self.meta_at(&format!("BAK/{}/{}", self.run, join(&self.prefix, rel)));
        self.ensure_dir(crate::util::parent_path(&to))?;
        match self.t().rename(from, &to) {
            Ok(()) => Ok(to),
            Err(e) => {
                // Taken already in this run: use a fresh timestamp instead.
                if self.t().stat(&to).is_err() {
                    return Err(e);
                }
                let to = self.meta_at(&format!("BAK/{}/{}", now_string(), join(&self.prefix, rel)));
                self.ensure_dir(crate::util::parent_path(&to))?;
                self.t().rename(from, &to)?;
                Ok(to)
            }
        }
    }

    /// Write the state if it changed (specs/state.md, "Writing"). Returns
    /// whether the state on disk now holds this run's lines.
    pub fn write_state(&self) -> bool {
        if self.dry_run {
            return false;
        }
        let _one = self.history.write_lock.lock().unwrap();
        let cutoff = now_micros() - (self.keep_del_days as i64) * 86_400 * 1_000_000;
        let body = {
            let g = self.history.inner.lock().unwrap();
            let body = match &g.outer {
                // Part of a larger tree: our lines replace that subtree's.
                Some(outer) => {
                    let mut merged: Tree = outer.iter().filter(|(d, _)| !under(d, &self.prefix)).map(|(d, l)| (d.clone(), l.clone())).collect();
                    for (d, lines) in &g.tree {
                        merged.insert(join(&self.prefix, d), lines.clone());
                    }
                    state::serialize_body(&merged, cutoff, "")
                }
                None => state::serialize_body(&g.tree, cutoff, ""),
            };
            if body == g.written_body {
                return true;
            }
            body
        };
        match write_state_files(self.t(), &self.up, now_micros(), &body) {
            Ok(()) => {
                self.history.inner.lock().unwrap().written_body = body;
                true
            }
            Err(e) => {
                output::error(&format!("state write failed for {}: {}", self.url, e));
                note_failure();
                false
            }
        }
    }

    /// Merge run-log lines into this peer's run log, oldest first.
    pub fn merge_runs(&self, text: &str) -> Result<()> {
        let path = self.meta_at(RUNS);
        let mut lines: Vec<String> = read_text(self.t(), &path)?.unwrap_or_default().lines().map(str::to_string).collect();
        lines.extend(text.lines().map(str::to_string));
        lines.retain(|l| !l.trim().is_empty());
        lines.sort();
        lines.dedup();
        let keep = if lines.len() > 1000 { &lines[lines.len() - 1000..] } else { &lines[..] };
        replace_meta_file(self.t(), &self.up, RUNS, (keep.join("\n") + "\n").as_bytes())
    }

}

/// See `Peer::tag`. Positions past 35 all print as `?`.
pub fn peer_tag(index: usize) -> char {
    match index + 1 {
        n @ 1..=9 => (b'0' + n as u8) as char,
        n @ 10..=35 => (b'a' + (n - 10) as u8) as char,
        _ => '?',
    }
}

pub fn meta(path: &str) -> String {
    join(META, path)
}

/// Whether `path` lies inside the BAK that holds this peer's displaced
/// entries (at the root, or at the ancestor that holds the state).
pub fn in_bak(path: &str) -> bool {
    let mut p = path;
    while let Some(rest) = p.strip_prefix("../") {
        p = rest;
    }
    p.starts_with(".kitchensync/BAK/")
}

/// Global count of failures that turn `sync complete` into exit code 2.
pub static FAILURES: AtomicUsize = AtomicUsize::new(0);

pub fn note_failure() {
    FAILURES.fetch_add(1, Ordering::SeqCst);
}

/// A peer's lines for the whole tree, held in memory for the run.
pub struct History {
    inner: Mutex<HistInner>,
    write_lock: Mutex<()>,
}

struct HistInner {
    tree: Tree,
    /// When the file this was read from was written.
    written: Option<i64>,
    /// The entry lines as last written (or read), to skip unchanged writes.
    written_body: String,
    /// The ancestor's whole tree, when this root takes its history from an
    /// ancestor; `tree` is then that tree's subtree, rebased here.
    outer: Option<Tree>,
}

fn under(dir: &str, prefix: &str) -> bool {
    prefix.is_empty() || dir == prefix || dir.strip_prefix(prefix).is_some_and(|r| r.starts_with('/'))
}

impl History {
    pub fn new(text: Option<&str>, keep_del_days: u64) -> History {
        let (written, tree) = text.map(state::parse_state).unwrap_or_default();
        let cutoff = now_micros() - (keep_del_days as i64) * 86_400 * 1_000_000;
        // An older format counts as changed, so the next write upgrades it.
        let current = text.is_some_and(|t| state::version(t) == state::FORMAT);
        let written_body = if current { state::serialize_body(&tree, cutoff, "") } else { String::new() };
        History { inner: Mutex::new(HistInner { tree, written, written_body, outer: None }), write_lock: Mutex::new(()) }
    }

    /// History taken from an ancestor's state: the subtree at `prefix`,
    /// rebased to this root. The whole tree is kept to write back.
    pub fn from_ancestor(text: &str, prefix: &str, keep_del_days: u64) -> History {
        let (written, outer) = state::parse_state(text);
        let cutoff = now_micros() - (keep_del_days as i64) * 86_400 * 1_000_000;
        let written_body = if state::version(text) == state::FORMAT { state::serialize_body(&outer, cutoff, "") } else { String::new() };
        let mut tree = Tree::new();
        for (d, lines) in outer.iter().filter(|(d, _)| under(d, prefix)) {
            let rel = if d == prefix { String::new() } else { d[prefix.len() + 1..].to_string() };
            tree.insert(rel, lines.clone());
        }
        History { inner: Mutex::new(HistInner { tree, written, written_body, outer: Some(outer) }), write_lock: Mutex::new(()) }
    }

    /// The lines for `dir`'s children.
    pub fn dir_lines(&self, dir: &str) -> DirLines {
        self.inner.lock().unwrap().tree.get(dir).cloned().unwrap_or_default()
    }

    pub fn line(&self, path: &str) -> Option<Line> {
        let g = self.inner.lock().unwrap();
        g.tree.get(crate::util::parent_path(path))?.get(crate::util::basename(path)).cloned()
    }

    pub fn has_dir_lines(&self, dir: &str) -> bool {
        self.inner.lock().unwrap().tree.get(dir).is_some_and(|l| !l.is_empty())
    }

    /// Replace one directory's lines.
    pub fn settle(&self, dir: &str, lines: DirLines) {
        let mut g = self.inner.lock().unwrap();
        if lines.is_empty() {
            g.tree.remove(dir);
        } else {
            g.tree.insert(dir.to_string(), lines);
        }
    }

    /// Forget everything beneath `path` (it no longer exists on this peer).
    pub fn drop_subtree(&self, path: &str) {
        let mut g = self.inner.lock().unwrap();
        g.tree.retain(|dir, _| !under(dir, path));
    }

    /// Drop the line for `path` and everything beneath it.
    pub fn forget(&self, path: &str) {
        let mut g = self.inner.lock().unwrap();
        let (dir, name) = (crate::util::parent_path(path), crate::util::basename(path));
        if let Some(lines) = g.tree.get_mut(dir) {
            lines.remove(name);
        }
        g.tree.retain(|d, _| !under(d, path));
    }

    pub fn set_line(&self, path: &str, line: Line) {
        let mut g = self.inner.lock().unwrap();
        g.tree.entry(crate::util::parent_path(path).to_string()).or_default().insert(crate::util::basename(path).to_string(), line);
    }

    /// Every live file line beneath `path`: (relative path, size, mod_time).
    pub fn files_under(&self, path: &str) -> Vec<(String, i64, i64)> {
        let g = self.inner.lock().unwrap();
        let mut out = Vec::new();
        for (dir, lines) in g.tree.iter().filter(|(dir, _)| under(dir, path)) {
            for (name, l) in lines {
                if !l.is_dir && l.deleted_time.is_none() {
                    out.push((join(dir, name), l.byte_size, l.mod_time));
                }
            }
        }
        out
    }

    /// Every live file line of at least `min` bytes.
    pub fn all_files(&self, min: i64) -> Vec<(String, i64, i64)> {
        let g = self.inner.lock().unwrap();
        let mut out = Vec::new();
        for (dir, lines) in &g.tree {
            for (name, l) in lines {
                if !l.is_dir && l.deleted_time.is_none() && l.byte_size >= min {
                    out.push((join(dir, name), l.byte_size, l.mod_time));
                }
            }
        }
        out
    }

    /// A nested sync root at `dir` (specs/state.md, "Nested sync roots"):
    /// use its lines when its state is newer than ours. Returns false when
    /// its format is newer than this KitchenSync reads (it is then left alone).
    pub fn nested_root(&self, dir: &str, text: &str) -> bool {
        if state::version(text) > state::FORMAT {
            output::error(&format!("{dir}/.kitchensync/state.txt has a newer format than this KitchenSync reads; its history is not used"));
            return false;
        }
        let (written, sub) = state::parse_state(text);
        let mut g = self.inner.lock().unwrap();
        let has_ours = g.tree.keys().any(|d| under(d, dir));
        if has_ours && written <= g.written {
            return true;
        }
        g.tree.retain(|d, _| !under(d, dir));
        for (d, lines) in sub {
            g.tree.insert(join(dir, &d), lines);
        }
        true
    }
}

struct Inner {
    lines: DirLines,
    original: DirLines,
    outstanding: usize,
    decided: bool,
    settled: bool,
}

/// One directory's lines on one peer, updated as decisions are made and
/// settled into the peer's history once the directory is decided and its
/// copies have finished (specs/multi-tree-sync.md, "State Updates").
pub struct DirState {
    pub peer: Arc<Peer>,
    pub dir: String,
    inner: Mutex<Inner>,
}

impl DirState {
    /// `original` is what the history holds for the directory when it
    /// differs from `lines` (lines read from a per-directory manifest).
    pub fn new(peer: Arc<Peer>, dir: &str, lines: DirLines, original: Option<DirLines>) -> Arc<DirState> {
        let original = original.unwrap_or_else(|| lines.clone());
        Arc::new(DirState { peer, dir: dir.to_string(), inner: Mutex::new(Inner { original, lines, outstanding: 0, decided: false, settled: false }) })
    }

    pub fn get(&self, name: &str) -> Option<Line> {
        self.inner.lock().unwrap().lines.get(name).cloned()
    }

    /// Entry confirmed present by a listing. An unchanged entry keeps its
    /// line as it is, `last_seen` included, so that an unchanged tree leaves
    /// the state as it was. `last_seen` is refreshed only when it is not
    /// already later than the entry's own mod_time.
    pub fn confirm_present(&self, name: &str, is_dir: bool, mod_time: i64, byte_size: i64) {
        let mut g = self.inner.lock().unwrap();
        if let Some(l) = g.lines.get(name) {
            let unchanged = l.deleted_time.is_none()
                && l.is_dir == is_dir
                && (is_dir || ((l.mod_time - mod_time).abs() <= 5_000_000 && l.byte_size == byte_size && l.last_seen.is_some_and(|ls| ls > mod_time + 5_000_000)));
            if unchanged {
                return;
            }
        }
        g.lines.insert(name.to_string(), Line { is_dir, mod_time, byte_size, last_seen: Some(now_micros()), deleted_time: None });
    }

    /// A directory KitchenSync just created here.
    pub fn created_dir(&self, name: &str) {
        let now = now_micros();
        self.inner.lock().unwrap().lines.insert(name.to_string(), Line { is_dir: true, mod_time: now, byte_size: -1, last_seen: Some(now), deleted_time: None });
    }

    /// Decision "push to this peer": intended state without `last_seen`. The
    /// peer has not been seen holding this version, so no time is kept: if
    /// the copy fails, the line can never read as a confirmed deletion.
    pub fn intend_push(&self, name: &str, mod_time: i64, byte_size: i64) {
        let mut g = self.inner.lock().unwrap();
        g.lines.insert(name.to_string(), Line { is_dir: false, mod_time, byte_size, last_seen: None, deleted_time: None });
        g.outstanding += 1;
    }

    /// A queued copy finished (successfully or not).
    pub fn copy_finished(&self, name: &str, ok: bool) {
        let settle = {
            let mut g = self.inner.lock().unwrap();
            if ok {
                if let Some(l) = g.lines.get_mut(name) {
                    l.last_seen = Some(now_micros());
                    l.deleted_time = None;
                }
            }
            g.outstanding -= 1;
            g.outstanding == 0 && g.decided
        };
        if settle {
            self.settle();
        }
    }

    /// Entry confirmed absent or displaced: tombstone with the line's own
    /// `last_seen`, or a fresh timestamp when it never had one. A directory's
    /// lines beneath it go.
    pub fn confirm_absent(&self, name: &str) {
        let was_dir = {
            let mut g = self.inner.lock().unwrap();
            match g.lines.get_mut(name) {
                Some(l) => {
                    if l.deleted_time.is_none() {
                        l.deleted_time = Some(l.last_seen.unwrap_or_else(now_micros));
                    }
                    l.is_dir
                }
                None => false,
            }
        };
        if was_dir {
            self.peer.history.drop_subtree(&join(&self.dir, name));
        }
    }

    /// All entries in this directory are decided; settle once copies finish.
    pub fn finish_deciding(&self) {
        let settle = {
            let mut g = self.inner.lock().unwrap();
            g.decided = true;
            g.outstanding == 0
        };
        if settle {
            self.settle();
        }
    }

    fn settle(&self) {
        let lines = {
            let mut g = self.inner.lock().unwrap();
            if g.settled {
                return;
            }
            g.settled = true;
            if g.lines == g.original {
                return;
            }
            g.lines.clone()
        };
        self.peer.history.settle(&self.dir, lines);
    }
}

pub const RUNS: &str = "runs.txt";

/// Append one line to `<root>/.kitchensync/runs.txt` (kept to the last 1000).
pub fn append_run(t: &dyn Transport, base: &str, line: &str) -> Result<()> {
    let path = join(base, &meta(RUNS));
    let mut text = read_text(t, &path)?.unwrap_or_default();
    text.push_str(line);
    text.push('\n');
    let lines: Vec<&str> = text.lines().collect();
    let keep = if lines.len() > 1000 { &lines[lines.len() - 1000..] } else { &lines[..] };
    let text = keep.join("\n") + "\n";
    replace_meta_file(t, base, RUNS, text.as_bytes())
}

/// Write a state in format 2: the lines to `state.gz`, then the one-line
/// marker to `state.txt` (specs/state.md, "State file").
fn write_state_files(t: &dyn Transport, dir: &str, written: i64, body: &str) -> Result<()> {
    replace_meta_file(t, dir, state::STATE_GZ, &state::gzip(&state::with_header(written, body)))?;
    replace_meta_file(t, dir, state::STATE, state::marker(written).as_bytes())
}

/// The state text at `<dir>/.kitchensync/`, or None when there is none.
/// Format 1 keeps it all in `state.txt`; format 2 keeps it compressed in
/// `state.gz` behind a one-line `state.txt`. A newer format's `state.txt` is
/// returned as it is, so the caller can see the version and refuse it.
/// `fallbacks` adds the `.new`/`.old` names a dry run reads instead of
/// repairing them.
pub fn read_state(t: &dyn Transport, dir: &str, fallbacks: bool) -> Result<Option<String>> {
    let base = join(dir, META);
    let suffixes: &[&str] = if fallbacks { &["", ".new", ".old"] } else { &[""] };
    let mut marker = None;
    for sfx in suffixes {
        marker = read_text(t, &join(&base, &format!("{}{sfx}", state::STATE)))?;
        if marker.is_some() {
            break;
        }
    }
    let Some(marker) = marker else { return Ok(None) };
    if state::version(&marker) != 2 {
        return Ok(Some(marker));
    }
    for sfx in suffixes {
        if let Some(bytes) = read_bytes(t, &join(&base, &format!("{}{sfx}", state::STATE_GZ)))? {
            return state::gunzip(&bytes).map(Some).map_err(|e| crate::transport::TransportError::io(format!("state.gz: {e}")));
        }
    }
    Err(crate::transport::TransportError::io("state.txt names format 2 but state.gz is missing"))
}

/// Start timestamp of the newest run recorded at the root, if any.
pub fn last_run_start(t: &dyn Transport, base: &str) -> Option<i64> {
    let text = read_text(t, &join(base, &meta(RUNS))).ok()??;
    text.lines().rev().find_map(|l| crate::util::parse_time(l.split('\t').next()?))
}

/// Replace `<dir>/.kitchensync/<name>` without renaming over a live file:
/// write `.new`, move live to `.old`, rename `.new` in, delete `.old`.
pub fn replace_meta_file(t: &dyn Transport, dir: &str, name: &str, data: &[u8]) -> Result<()> {
    let live = join(&join(dir, META), name);
    let new = format!("{live}.new");
    let old = format!("{live}.old");
    let mut w = t.open_write(&new)?;
    w.write_all(data)?;
    w.close()?;
    let has_live = exists(t, &live)?;
    if has_live {
        t.rename(&live, &old)?;
    }
    t.rename(&new, &live)?;
    if has_live {
        t.delete_file(&old)?;
    }
    Ok(())
}

fn exists(t: &dyn Transport, p: &str) -> Result<bool> {
    match t.stat(p) {
        Ok(_) => Ok(true),
        Err(e) if e.is_not_found() => Ok(false),
        Err(e) => Err(e),
    }
}

/// Repair an interrupted replacement of `<root>/.kitchensync/<name>`
/// (specs/state.md, "Writing").
pub fn recover_meta_file(t: &dyn Transport, base: &str, name: &str) -> Result<()> {
    let live = join(base, &meta(name));
    let new = format!("{live}.new");
    let old = format!("{live}.old");
    let has_old = exists(t, &old)?;
    let has_new = exists(t, &new)?;
    if !has_old && !has_new {
        return Ok(());
    }
    let has_live = exists(t, &live)?;
    match (has_old, has_new, has_live) {
        (true, _, true) => {
            if has_new {
                t.delete_file(&new)?;
            }
            t.delete_file(&old)?;
        }
        (true, true, false) => {
            t.rename(&new, &live)?;
            t.delete_file(&old)?;
        }
        (true, false, false) => t.rename(&old, &live)?,
        (false, true, true) => t.delete_file(&new)?,
        (false, true, false) => t.rename(&new, &live)?,
        (false, false, _) => {}
    }
    Ok(())
}

/// Read `<root>/.kitchensync/ignore`, or None when absent.
pub fn read_ignore(t: &dyn Transport) -> Result<Option<String>> {
    read_text(t, &meta("ignore"))
}

/// Read a whole text file, or None when it does not exist.
pub fn read_text(t: &dyn Transport, path: &str) -> Result<Option<String>> {
    Ok(read_bytes(t, path)?.map(|b| String::from_utf8_lossy(&b).into_owned()))
}

/// Read a whole file, or None when it does not exist.
pub fn read_bytes(t: &dyn Transport, path: &str) -> Result<Option<Vec<u8>>> {
    let mut r = match t.open_read(path) {
        Ok(r) => r,
        Err(e) if e.is_not_found() => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = r.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(Some(buf))
}
