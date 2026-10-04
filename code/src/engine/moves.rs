//! Files a peer holds or recently held, so a copy can move one into place
//! instead of transferring it. See specs/sync.md, "Moved Files".

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use super::peer::in_bak;

/// Copies of files at least this large look for a file to move instead.
pub const MOVE_MIN: i64 = 1 << 20;

const TOL: i64 = 5_000_000; // 5 seconds in microseconds

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Use {
    Free,
    Reserved,
    Used,
}

struct Cand {
    path: String,
    size: i64,
    mod_time: i64,
    state: Use,
    /// The walk wanted it displaced while it was reserved: the copy that
    /// holds it displaces it if it does not use it.
    doomed: bool,
}

#[derive(Default)]
struct Inner {
    cands: Vec<Cand>,
    by_path: HashMap<String, usize>,
    /// Live paths whose file was moved elsewhere in this run.
    moved_away: HashSet<String>,
}

/// One peer's candidates. `fs` serializes displacements with moves on the
/// peer, so a move always renames from the candidate's current location.
#[derive(Default)]
pub struct Index {
    inner: Mutex<Inner>,
    fs: Mutex<()>,
}

fn matches(c: &Cand, size: i64, mod_time: i64, not_path: &str) -> bool {
    c.size == size && (c.mod_time - mod_time).abs() <= TOL && c.path != not_path
}

impl Index {
    pub fn lock_fs(&self) -> MutexGuard<'_, ()> {
        self.fs.lock().unwrap()
    }

    /// Add a file the peer holds (at a live path or in BAK).
    pub fn add(&self, path: String, size: i64, mod_time: i64) {
        if size < MOVE_MIN {
            return;
        }
        let mut g = self.inner.lock().unwrap();
        if g.by_path.contains_key(&path) {
            return;
        }
        let i = g.cands.len();
        g.by_path.insert(path.clone(), i);
        g.cands.push(Cand { path, size, mod_time, state: Use::Free, doomed: false });
    }

    /// Reserve the first free candidate, other than those in `skip`, that
    /// matches and that is in BAK or accepted by `ok`.
    pub fn reserve(&self, size: i64, mod_time: i64, not_path: &str, skip: &[usize], ok: impl Fn(&str) -> bool) -> Option<usize> {
        let mut g = self.inner.lock().unwrap();
        let i = g.cands.iter().enumerate().position(|(i, c)| c.state == Use::Free && !skip.contains(&i) && matches(c, size, mod_time, not_path) && (in_bak(&c.path) || ok(&c.path)))?;
        g.cands[i].state = Use::Reserved;
        Some(i)
    }

    pub fn path(&self, id: usize) -> String {
        self.inner.lock().unwrap().cands[id].path.clone()
    }

    /// Give a reservation back. Returns whether the walk wanted the file
    /// displaced meanwhile (the caller must displace it now).
    pub fn release(&self, id: usize) -> bool {
        let mut g = self.inner.lock().unwrap();
        let c = &mut g.cands[id];
        c.state = Use::Free;
        std::mem::take(&mut c.doomed)
    }

    /// The candidate was moved into place. Call with `lock_fs` held.
    pub fn used(&self, id: usize) {
        let mut g = self.inner.lock().unwrap();
        let path = g.cands[id].path.clone();
        g.cands[id].state = Use::Used;
        g.by_path.remove(&path);
        if !in_bak(&path) {
            g.moved_away.insert(path);
        }
    }

    /// Before the walk displaces `rel` (with `lock_fs` held): true when the
    /// displacement must be skipped, because the file was already moved away
    /// or a copy holds it (which then displaces it if unused).
    pub fn skip_displacement(&self, rel: &str) -> bool {
        let mut g = self.inner.lock().unwrap();
        if g.moved_away.contains(rel) {
            return true;
        }
        let Some(&i) = g.by_path.get(rel) else { return false };
        if g.cands[i].state == Use::Reserved {
            g.cands[i].doomed = true;
            return true;
        }
        false
    }

    /// `rel` (a file or a directory) moved to `to` in BAK: candidates at or
    /// beneath it follow it. Call with `lock_fs` held.
    pub fn displaced(&self, rel: &str, to: &str) {
        let mut g = self.inner.lock().unwrap();
        let prefix = format!("{rel}/");
        let hits: Vec<usize> = g.by_path.iter().filter(|(p, _)| *p == rel || p.starts_with(&prefix)).map(|(_, i)| *i).collect();
        for i in hits {
            let old = g.cands[i].path.clone();
            let new = format!("{to}{}", &old[rel.len()..]);
            g.by_path.remove(&old);
            g.by_path.insert(new.clone(), i);
            g.cands[i].path = new;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_follow_displacement_and_reservations_hold() {
        let ix = Index::default();
        ix.add("d/a.bin".into(), MOVE_MIN, 10_000_000);
        ix.add("small".into(), 10, 10_000_000);
        // Not at its own path, and the in-memory check is honored.
        assert_eq!(ix.reserve(MOVE_MIN, 10_000_000, "d/a.bin", &[], |_| true), None);
        assert_eq!(ix.reserve(MOVE_MIN, 12_000_000, "x", &[], |_| false), None);
        let id = ix.reserve(MOVE_MIN, 12_000_000, "x", &[], |_| true).unwrap();
        // The walk wants it gone while it is reserved: skipped, and doomed.
        assert!(ix.skip_displacement("d/a.bin"));
        assert!(ix.release(id));
        assert!(!ix.skip_displacement("d/a.bin"));
        // A displaced directory takes its candidates with it.
        ix.displaced("d", ".kitchensync/BAK/t/d");
        let id = ix.reserve(MOVE_MIN, 10_000_000, "x", &[], |_| false).unwrap();
        assert_eq!(ix.path(id), ".kitchensync/BAK/t/d/a.bin");
        ix.used(id);
        assert_eq!(ix.reserve(MOVE_MIN, 10_000_000, "x", &[], |_| true), None);
    }
}
