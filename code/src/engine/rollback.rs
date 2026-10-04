//! `--rollback <timestamp>` and `--undo`: undo journal lines newest first.
//! See specs/state.md, "Rollback", and specs/sync.md, "Rollback".

use crate::config::Config;
use crate::output;
use crate::state::{self, JLine, Line};
use crate::transport::{join, Entry, Transport};
use crate::util::{now_micros, parse_time, system_to_micros};

use super::fsops::{self, exists, PeerRef};
use super::peer::{self, in_bak, note_failure, Peer};

const TOL: i64 = 5_000_000;

pub fn run(cfg: &Config, peers: &[PeerRef], ts: Option<i64>) -> i32 {
    if peers.is_empty() {
        output::error("no peer is reachable");
        return 1;
    }
    for p in peers {
        let Some((lines, newest)) = read_journals(p) else {
            output::error(&format!("rollback skipped for {}: a journal there has a newer format than this KitchenSync reads", p.url));
            note_failure();
            continue;
        };
        let target = match ts.or_else(|| peer::last_run_start(p.t(), &p.up)).or(newest.map(|t| t - 1)) {
            Some(t) => t,
            None => {
                output::line(&format!("nothing to undo for {}", p.url));
                continue;
            }
        };
        let mut todo: Vec<&JLine> = lines.iter().filter(|l| l.ts > target && l.op != 'B').collect();
        todo.sort_by_key(|l| std::cmp::Reverse(l.ts));
        let r = Rollback { cfg, peer: p };
        for l in todo {
            r.undo(l);
        }
        p.write_state();
        p.close_journal();
    }
    super::finish("rollback complete")
}

/// Every journal line on the peer, and the newest journal's start time; None
/// when a journal has a format this KitchenSync does not read.
fn read_journals(p: &Peer) -> Option<(Vec<JLine>, Option<i64>)> {
    let dir = p.meta_at("journal");
    let Ok(entries) = p.t().list_dir(&dir) else { return Some((Vec::new(), None)) };
    let mut lines = Vec::new();
    let mut newest = None;
    for e in entries.iter().filter(|e| !e.is_dir) {
        newest = newest.max(parse_time(e.name.strip_suffix(".txt").unwrap_or(&e.name)));
        if let Ok(Some(text)) = peer::read_text(p.t(), &join(&dir, &e.name)) {
            if state::version(&text) > state::FORMAT {
                return None;
            }
            // Journal paths are relative to the folder that holds the
            // state; keep the lines inside this sync root, in its terms.
            for mut l in state::parse_journal(&text) {
                let Some(path) = p.from_anchor(&l.path) else { continue };
                let other = match &l.other {
                    Some(o) => match p.from_anchor(o) {
                        Some(o) => Some(o),
                        None => continue,
                    },
                    None => None,
                };
                l.path = path;
                l.other = other;
                lines.push(l);
            }
        }
    }
    Some((lines, newest))
}

struct Rollback<'a> {
    cfg: &'a Config,
    peer: &'a PeerRef,
}

impl Rollback<'_> {
    fn t(&self) -> &dyn Transport {
        self.peer.t()
    }

    fn fail(&self, what: &str, rel: &str, e: &dyn std::fmt::Display) {
        output::error(&format!("rollback: {} failed for {} on {}: {}", what, rel, self.peer.url, e));
        note_failure();
    }

    /// The live entry at `path`, if any.
    fn live(&self, path: &str) -> Option<Entry> {
        self.t().stat(path).ok()
    }

    fn same_file(&self, path: &str, l: &JLine) -> Option<Entry> {
        self.live(path).filter(|e| !e.is_dir && Some(e.byte_size) == l.byte_size && l.mod_time.is_some_and(|m| (system_to_micros(e.mod_time) - m).abs() <= TOL))
    }

    fn undo(&self, l: &JLine) {
        let dry = self.cfg.dry_run;
        let tag = self.peer.tag();
        match l.op {
            // Content KitchenSync put in place: take it away, unless the user
            // has changed it since.
            'C' | 'D' => {
                let entry = if l.op == 'C' { self.same_file(&l.path, l) } else { self.live(&l.path).filter(|e| e.is_dir) };
                let Some(entry) = entry else { return };
                output::info(&format!("X{tag} {}", l.path));
                if !dry && fsops::displace(self.peer, &l.path, Some(&entry), false) {
                    self.peer.history.forget(&l.path);
                }
            }
            // A file KitchenSync moved here: send it back.
            'M' => {
                let Some(other) = &l.other else { return };
                if self.same_file(&l.path, l).is_none() {
                    return;
                }
                if !in_bak(other) {
                    output::info(&format!("R{tag} {other}"));
                }
                if dry {
                    return;
                }
                if exists(self.t(), other).unwrap_or(true) {
                    self.fail("return", &l.path, &format!("{other} is occupied"));
                    return;
                }
                if let Err(e) = self.put(&l.path, other) {
                    self.fail("return", &l.path, &e);
                    return;
                }
                self.peer.history.forget(&l.path);
                if !in_bak(other) {
                    self.restored(other);
                }
            }
            // An entry KitchenSync displaced: bring it back.
            'X' => {
                let Some(other) = &l.other else { return };
                output::info(&format!("R{tag} {}", l.path));
                if dry {
                    return;
                }
                if !exists(self.t(), other).unwrap_or(false) {
                    self.fail("restore", &l.path, &format!("{other} is gone"));
                    return;
                }
                if let Some(e) = self.live(&l.path) {
                    if !fsops::displace(self.peer, &l.path, Some(&e), false) {
                        return;
                    }
                }
                if let Err(e) = self.put(other, &l.path) {
                    self.fail("restore", &l.path, &e);
                    return;
                }
                self.restored(&l.path);
            }
            _ => {}
        }
    }

    /// Rename `from` to `to` (making `to`'s parents) and journal it.
    fn put(&self, from: &str, to: &str) -> crate::transport::Result<()> {
        let parent = crate::util::parent_path(to);
        if !parent.is_empty() {
            self.peer.ensure_dir(parent)?;
        }
        self.t().rename(from, to)?;
        let e = self.live(to);
        let file = e.filter(|e| !e.is_dir);
        self.peer.journal('M', to, Some(from), file.as_ref().map(|e| e.byte_size), file.as_ref().map(|e| system_to_micros(e.mod_time)));
        Ok(())
    }

    /// A restored entry gets a fresh present line; what was beneath it is
    /// unknown, so the peer has no opinion there.
    fn restored(&self, path: &str) {
        self.peer.history.forget(path);
        if let Some(e) = self.live(path) {
            let line = Line { is_dir: e.is_dir, mod_time: system_to_micros(e.mod_time), byte_size: if e.is_dir { -1 } else { e.byte_size }, last_seen: Some(now_micros()), deleted_time: None };
            self.peer.history.set_line(path, line);
        }
    }
}
