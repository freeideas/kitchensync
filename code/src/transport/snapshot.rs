//! A whole-tree listing streamed from a peer's server, handed to the walk one
//! directory at a time. See specs/sync.md, "Listing A Whole Tree".
//!
//! The lister sends each directory's entries followed by an end record, in
//! the order the walk visits directories. A directory is served from the
//! stream at most once, and only to the first listing of it: anything listed
//! again, or already listed before its records arrived, is asked of the
//! server, so a directory KitchenSync has changed is never answered from the
//! stream. A listing that asks for a directory the stream has not reached yet
//! waits for it, so the disk is read once rather than by the lister and the
//! walk at the same time; it does not wait for a directory the stream will
//! never send (one created during the run, or one whose parent's listing has
//! ended without it).

use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use super::Entry;

/// Give up waiting on a stream that has sent nothing for this long.
const STALL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct State {
    /// A lister is (or may soon be) sending records.
    running: bool,
    /// Entries received for directories whose end record has not come yet.
    partial: HashMap<String, Vec<Entry>>,
    /// Complete listings not yet handed out.
    ready: HashMap<String, Vec<Entry>>,
    /// Directories the stream has named as a subdirectory (and the root).
    announced: HashSet<String>,
    /// Directories whose end record has arrived.
    ended: HashSet<String>,
    /// Directories already listed once, from the stream or not.
    listed: HashSet<String>,
    /// Bytes of a record cut off at the end of the last chunk.
    tail: Vec<u8>,
    last_progress: Option<Instant>,
    directories: usize,
}

#[derive(Default)]
pub struct Snapshot {
    state: Mutex<State>,
    changed: Condvar,
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

impl Snapshot {
    /// A lister is about to run: listings wait for its records from now on.
    pub fn start(&self) {
        let mut g = self.state.lock().unwrap();
        g.running = true;
        g.announced.insert(String::new());
        g.last_progress = Some(Instant::now());
    }

    /// Records arrived from the lister.
    pub fn feed(&self, bytes: &[u8]) {
        let mut g = self.state.lock().unwrap();
        let mut data = std::mem::take(&mut g.tail);
        data.extend_from_slice(bytes);
        let mut records = data.split(|b| *b == 0).collect::<Vec<_>>();
        let tail = records.pop().unwrap_or_default().to_vec();
        for rec in records {
            let rec = String::from_utf8_lossy(rec);
            let mut f = rec.splitn(4, '\t');
            let (Some(kind), Some(size), Some(mtime), Some(path)) = (f.next(), f.next(), f.next(), f.next()) else { continue };
            if kind == "e" {
                let entries = g.partial.remove(path).unwrap_or_default();
                if !g.listed.contains(path) {
                    g.ready.insert(path.to_string(), entries);
                }
                g.ended.insert(path.to_string());
                g.directories += 1;
                continue;
            }
            let (Ok(size), Ok(mtime)) = (size.parse::<i64>(), mtime.parse::<f64>()) else { continue };
            let is_dir = kind == "d";
            let (parent, name) = match path.rsplit_once('/') {
                Some((p, n)) => (p.to_string(), n.to_string()),
                None => (String::new(), path.to_string()),
            };
            if is_dir && name != ".kitchensync" && name != ".git" {
                g.announced.insert(path.to_string());
            }
            let mod_time = UNIX_EPOCH + Duration::from_secs_f64(mtime.max(0.0));
            g.partial.entry(parent).or_default().push(Entry { name, is_dir, mod_time, byte_size: if is_dir { -1 } else { size } });
        }
        g.tail = tail;
        g.last_progress = Some(Instant::now());
        self.changed.notify_all();
    }

    /// The lister is done (or could not run): nothing more will arrive.
    /// Returns how many directories it listed completely.
    pub fn finish(&self) -> usize {
        let mut g = self.state.lock().unwrap();
        g.running = false;
        g.partial.clear();
        self.changed.notify_all();
        g.directories
    }

    /// Whether anything has arrived since `start`.
    pub fn received_any(&self) -> bool {
        let g = self.state.lock().unwrap();
        g.directories > 0 || !g.partial.is_empty()
    }

    /// Whether `path` may still arrive: it has been announced, or its nearest
    /// ancestor the stream has reached is still being listed.
    fn may_come(g: &State, path: &str) -> bool {
        let mut at = path;
        loop {
            if g.ended.contains(at) {
                return false;
            }
            if g.announced.contains(at) {
                return true;
            }
            if at.is_empty() {
                return false;
            }
            let parent = parent_of(at);
            if g.ended.contains(parent) {
                return false;
            }
            at = parent;
        }
    }

    /// The first listing of `path`, if the stream has it or will send it;
    /// None means "ask the server".
    pub fn take(&self, path: &str) -> Option<Vec<Entry>> {
        let mut g = self.state.lock().unwrap();
        if g.listed.contains(path) {
            return None;
        }
        loop {
            if let Some(entries) = g.ready.remove(path) {
                g.listed.insert(path.to_string());
                return Some(entries);
            }
            if !g.running || !Self::may_come(&g, path) || g.last_progress.is_some_and(|t| t.elapsed() > STALL) {
                // Listed by the server from now on: drop the stream's copy.
                g.listed.insert(path.to_string());
                return None;
            }
            g = self.changed.wait_timeout(g, Duration::from_secs(1)).unwrap().0;
        }
    }

    /// Whether a complete listing of `dir` is waiting to be handed out.
    #[cfg(test)]
    pub fn has(&self, dir: &str) -> bool {
        self.state.lock().unwrap().ready.contains_key(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: Vec<Entry>) -> Vec<String> {
        v.into_iter().map(|e| e.name).collect()
    }

    #[test]
    fn directories_are_served_once_as_they_complete() {
        let s = Snapshot::default();
        s.start();
        // A record split across two chunks.
        s.feed(b"d\t-1\t1700000000.5\ta\0f\t5\t17000");
        s.feed(b"00001\tx\ty.txt\0d\t-1\t1\t.kitchensync\0e\t-\t-\t\0");
        assert_eq!(names(s.take("").unwrap()), vec!["a", "x\ty.txt", ".kitchensync"]);
        assert_eq!(s.take(""), None, "a directory is served once");
        s.feed(b"d\t-1\t2\ta/empty\0e\t-\t-\ta\0e\t-\t-\ta/empty\0");
        assert_eq!(names(s.take("a").unwrap()), vec!["empty"]);
        assert_eq!(s.take("a/empty"), Some(vec![]));
        // Never named in a finished listing: not waited for.
        assert_eq!(s.take("new"), None);
        assert_eq!(s.take("a/new/deeper"), None);
        assert!(!s.has(".kitchensync"));
        assert_eq!(s.finish(), 3);
    }

    #[test]
    fn a_listing_waits_for_a_directory_still_to_come() {
        let s = std::sync::Arc::new(Snapshot::default());
        s.start();
        s.feed(b"d\t-1\t1\tlater\0e\t-\t-\t\0");
        let waiter = {
            let s = std::sync::Arc::clone(&s);
            std::thread::spawn(move || s.take("later"))
        };
        std::thread::sleep(Duration::from_millis(200));
        s.feed(b"f\t3\t1\tlater/f.txt\0e\t-\t-\tlater\0");
        assert_eq!(names(waiter.join().unwrap().unwrap()), vec!["f.txt"]);
    }

    #[test]
    fn listed_before_its_records_arrive_means_asked_of_the_server() {
        let s = Snapshot::default();
        s.start();
        s.finish();
        assert_eq!(s.take(""), None, "nothing will arrive once the lister is done");
        s.start();
        s.feed(b"e\t-\t-\t\0");
        assert!(!s.has(""), "already listed: its records are dropped");
    }
}
