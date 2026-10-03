//! Files displaced during this run that a copy can reuse instead of
//! transferring. See specs/sync.md, "Reusing A Displaced File".

use std::collections::HashMap;
use std::sync::Mutex;

use crate::transport::{join, Transport};
use crate::util::system_to_micros;

/// Copies of files at least this large are held until the walk ends and may
/// reuse a displaced file; smaller files are copied as found.
pub const HOLD_MIN: i64 = 1 << 20;

const TOL: i64 = 5_000_000; // 5 seconds in microseconds

struct Candidate {
    path: String,
    size: i64,
    mod_time: i64,
    used: bool,
}

#[derive(Default)]
struct PeerCandidates {
    files: Vec<Candidate>,
    /// Displaced directories, listed for files only when a held copy needs them.
    dirs: Vec<String>,
}

#[derive(Default)]
pub struct Reuse {
    peers: Mutex<HashMap<usize, PeerCandidates>>,
}

impl Reuse {
    /// A file displaced on `peer` to `path`.
    pub fn note_file(&self, peer: usize, path: String, size: i64, mod_time: i64) {
        if size >= HOLD_MIN {
            self.peers.lock().unwrap().entry(peer).or_default().files.push(Candidate { path, size, mod_time, used: false });
        }
    }

    /// A directory displaced on `peer` to `path`.
    pub fn note_dir(&self, peer: usize, path: String) {
        self.peers.lock().unwrap().entry(peer).or_default().dirs.push(path);
    }

    /// List the directories displaced on `peer` and add the files inside.
    pub fn expand(&self, peer: usize, t: &dyn Transport) {
        let dirs = match self.peers.lock().unwrap().get_mut(&peer) {
            Some(c) => std::mem::take(&mut c.dirs),
            None => return,
        };
        let mut found = Vec::new();
        for d in dirs {
            collect(t, &d, &mut found);
        }
        let mut g = self.peers.lock().unwrap();
        let c = g.entry(peer).or_default();
        c.files.extend(found);
    }

    /// The first unused candidate on `peer` with this size and time that
    /// `same_content` accepts. It is marked used; candidates that fail the
    /// check stay available for other copies.
    pub fn take(&self, peer: usize, size: i64, mod_time: i64, same_content: impl Fn(&str) -> bool) -> Option<String> {
        let mut tried: Vec<usize> = Vec::new();
        loop {
            let (i, path) = {
                let mut g = self.peers.lock().unwrap();
                let files = &mut g.get_mut(&peer)?.files;
                let i = files.iter().enumerate().position(|(i, c)| !c.used && !tried.contains(&i) && c.size == size && (c.mod_time - mod_time).abs() <= TOL)?;
                files[i].used = true;
                (i, files[i].path.clone())
            };
            if same_content(&path) {
                return Some(path);
            }
            tried.push(i);
            if let Some(c) = self.peers.lock().unwrap().get_mut(&peer) {
                c.files[i].used = false;
            }
        }
    }
}

/// Every file of at least `HOLD_MIN` bytes under `dir`, skipping KitchenSync's
/// own metadata. Unreadable directories are skipped: a missed candidate only
/// means a transfer.
fn collect(t: &dyn Transport, dir: &str, out: &mut Vec<Candidate>) {
    let Ok(entries) = t.list_dir(dir) else { return };
    for e in entries {
        if e.name == ".kitchensync" {
            continue;
        }
        let path = join(dir, &e.name);
        if e.is_dir {
            collect(t, &path, out);
        } else if e.byte_size >= HOLD_MIN {
            out.push(Candidate { path, size: e.byte_size, mod_time: system_to_micros(e.mod_time), used: false });
        }
    }
}
