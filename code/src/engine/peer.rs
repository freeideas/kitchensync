//! A reachable peer and the per-directory manifest state kept during a run.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::Role;
use crate::manifest::{self, Line};
use crate::output;
use crate::transport::{join, Transport};
use crate::util::{now_micros, now_string};

pub struct Peer {
    pub index: usize,
    pub role: Role,
    /// Normalized URL of the winning connection (for diagnostics).
    pub url: String,
    pub transport: Arc<dyn Transport>,
    /// Whether the sync root had a manifest at startup.
    pub had_history: bool,
}

impl Peer {
    pub fn is_canon(&self) -> bool {
        self.role == Role::Canon
    }
    pub fn contributes(&self) -> bool {
        self.role != Role::Subordinate
    }
}

pub const META: &str = ".kitchensync";

pub fn manifest_path(dir: &str) -> String {
    join(&join(dir, META), manifest::NAME)
}

/// Global count of failures that turn `sync complete` into exit code 2.
pub static FAILURES: AtomicUsize = AtomicUsize::new(0);

pub fn note_failure() {
    FAILURES.fetch_add(1, Ordering::SeqCst);
}

struct Inner {
    lines: BTreeMap<String, Line>,
    original: String,
    outstanding: usize,
    decided: bool,
    written: bool,
}

/// One directory's manifest on one peer, updated as decisions are made and
/// written once the directory is decided and its copies have finished.
pub struct DirState {
    pub peer: Arc<Peer>,
    pub dir: String,
    dry_run: bool,
    keep_del_days: u64,
    inner: Mutex<Inner>,
}

impl DirState {
    pub fn new(peer: Arc<Peer>, dir: &str, original: Option<String>, dry_run: bool, keep_del_days: u64) -> Arc<DirState> {
        let lines = original.as_deref().map(manifest::parse).unwrap_or_default();
        Arc::new(DirState {
            peer,
            dir: dir.to_string(),
            dry_run,
            keep_del_days,
            inner: Mutex::new(Inner { lines, original: original.unwrap_or_default(), outstanding: 0, decided: false, written: false }),
        })
    }

    pub fn get(&self, name: &str) -> Option<Line> {
        self.inner.lock().unwrap().lines.get(name).cloned()
    }

    /// Entry confirmed present by a listing. `placed` survives only when the
    /// entry is unchanged since KitchenSync put it there.
    pub fn confirm_present(&self, name: &str, is_dir: bool, mod_time: i64, byte_size: i64) {
        let mut g = self.inner.lock().unwrap();
        let placed = g.lines.get(name).filter(|l| l.deleted_time.is_none() && (is_dir || ((l.mod_time - mod_time).abs() <= 5_000_000 && l.byte_size == byte_size))).and_then(|l| l.placed);
        g.lines.insert(name.to_string(), Line { is_dir, mod_time, byte_size, last_seen: Some(now_micros()), deleted_time: None, placed });
    }

    /// A directory KitchenSync just created here.
    pub fn created_dir(&self, name: &str) {
        let mut g = self.inner.lock().unwrap();
        let now = now_micros();
        g.lines.insert(name.to_string(), Line { is_dir: true, mod_time: now, byte_size: -1, last_seen: Some(now), deleted_time: None, placed: Some(now) });
    }

    /// Decision "push to this peer": intended state without `last_seen`.
    pub fn intend_push(&self, name: &str, mod_time: i64, byte_size: i64) {
        let mut g = self.inner.lock().unwrap();
        let last_seen = g.lines.get(name).and_then(|l| l.last_seen);
        g.lines.insert(name.to_string(), Line { is_dir: false, mod_time, byte_size, last_seen, deleted_time: None, placed: None });
        g.outstanding += 1;
    }

    /// A queued copy finished (successfully or not).
    pub fn copy_finished(self: &Arc<Self>, name: &str, ok: bool) {
        let write = {
            let mut g = self.inner.lock().unwrap();
            if ok {
                if let Some(l) = g.lines.get_mut(name) {
                    let now = now_micros();
                    l.last_seen = Some(now);
                    l.deleted_time = None;
                    l.placed = Some(now);
                }
            }
            g.outstanding -= 1;
            g.outstanding == 0 && g.decided
        };
        if write {
            self.write();
        }
    }

    /// Entry confirmed absent or displaced: tombstone with the line's own
    /// `last_seen`, or a fresh timestamp when it never had one.
    pub fn confirm_absent(&self, name: &str) {
        let mut g = self.inner.lock().unwrap();
        if let Some(l) = g.lines.get_mut(name) {
            if l.deleted_time.is_none() {
                l.deleted_time = Some(l.last_seen.unwrap_or_else(now_micros));
            }
        }
    }

    /// All entries in this directory are decided; write once copies finish.
    pub fn finish_deciding(self: &Arc<Self>) {
        let write = {
            let mut g = self.inner.lock().unwrap();
            g.decided = true;
            g.outstanding == 0
        };
        if write {
            self.write();
        }
    }

    fn write(&self) {
        if self.dry_run {
            return;
        }
        let text = {
            let mut g = self.inner.lock().unwrap();
            if g.written {
                return;
            }
            g.written = true;
            let cutoff = now_micros() - (self.keep_del_days as i64) * 86_400 * 1_000_000;
            let text = manifest::serialize(&g.lines, cutoff);
            if text == g.original {
                return;
            }
            text
        };
        if let Err(e) = write_manifest(self.peer.transport.as_ref(), &self.dir, &text) {
            output::error(&format!("manifest write failed for {} at {}: {}", self.peer.url, self.dir, e));
            note_failure();
        }
    }
}

/// Write `.new`, move live to `.old`, rename `.new` in, archive `.old` to BAK.
pub fn write_manifest(t: &dyn Transport, dir: &str, text: &str) -> crate::transport::Result<()> {
    replace_meta_file(t, dir, manifest::NAME, text, true)
}

pub const RUNS: &str = "runs.txt";

/// Append one line to `<root>/.kitchensync/runs.txt` (kept to the last 1000).
pub fn append_run(t: &dyn Transport, line: &str) -> crate::transport::Result<()> {
    let path = join(META, RUNS);
    let mut text = read_text(t, &path)?.unwrap_or_default();
    text.push_str(line);
    text.push('\n');
    let lines: Vec<&str> = text.lines().collect();
    let keep = if lines.len() > 1000 { &lines[lines.len() - 1000..] } else { &lines[..] };
    let text = keep.join("\n") + "\n";
    replace_meta_file(t, "", RUNS, &text, false)
}

/// Start timestamp of the newest run recorded at the root, if any.
pub fn last_run_start(t: &dyn Transport) -> Option<i64> {
    let text = read_text(t, &join(META, RUNS)).ok()??;
    text.lines().rev().find_map(|l| crate::util::parse_time(l.split('\t').next()?))
}

/// Replace `<dir>/.kitchensync/<name>` without renaming over a live file.
fn replace_meta_file(t: &dyn Transport, dir: &str, name: &str, text: &str, archive: bool) -> crate::transport::Result<()> {
    let live = join(&join(dir, META), name);
    let new = format!("{live}.new");
    let old = format!("{live}.old");
    let mut w = t.open_write(&new)?;
    w.write_all(text.as_bytes())?;
    w.close()?;
    let has_live = match t.stat(&live) {
        Ok(_) => true,
        Err(e) if e.is_not_found() => false,
        Err(e) => return Err(e),
    };
    if has_live {
        t.rename(&live, &old)?;
    }
    t.rename(&new, &live)?;
    if has_live {
        if archive {
            archive_old_manifest(t, dir, &old)?;
        } else {
            t.delete_file(&old)?;
        }
    }
    Ok(())
}

fn archive_old_manifest(t: &dyn Transport, dir: &str, old: &str) -> crate::transport::Result<()> {
    let bak = join(&join(dir, META), &format!("BAK/{}", now_string()));
    t.create_dir(&bak)?;
    t.rename(old, &join(&bak, manifest::NAME))
}

/// Repair an interrupted manifest replacement (specs/manifest.md, "Writing").
pub fn recover_manifest(t: &dyn Transport, dir: &str) -> crate::transport::Result<()> {
    let live = manifest_path(dir);
    let new = format!("{live}.new");
    let old = format!("{live}.old");
    let ex = |p: &str| match t.stat(p) {
        Ok(_) => Ok(true),
        Err(e) if e.is_not_found() => Ok(false),
        Err(e) => Err(e),
    };
    let has_old = ex(&old)?;
    if !has_old && !ex(&new)? {
        return Ok(());
    }
    let has_new = ex(&new)?;
    let has_live = ex(&live)?;
    match (has_old, has_new, has_live) {
        (true, _, true) => {
            if has_new {
                t.delete_file(&new)?;
            }
            archive_old_manifest(t, dir, &old)?;
        }
        (true, true, false) => {
            t.rename(&new, &live)?;
            archive_old_manifest(t, dir, &old)?;
        }
        (true, false, false) => t.rename(&old, &live)?,
        (false, true, true) => t.delete_file(&new)?,
        (false, true, false) => t.rename(&new, &live)?,
        (false, false, _) => {}
    }
    Ok(())
}

/// Read a directory's manifest text, or None when it does not exist.
pub fn read_manifest(t: &dyn Transport, dir: &str) -> crate::transport::Result<Option<String>> {
    read_text(t, &manifest_path(dir))
}

/// Read `<root>/.kitchensync/ignore`, or None when absent.
pub fn read_ignore(t: &dyn Transport) -> crate::transport::Result<Option<String>> {
    read_text(t, &join(META, "ignore"))
}

fn read_text(t: &dyn Transport, path: &str) -> crate::transport::Result<Option<String>> {
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
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}
