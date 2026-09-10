//! `--rollback <timestamp>` and `--undo`. See specs/manifest.md, "Rollback".

use std::collections::BTreeMap;

use crate::config::Config;
use crate::manifest::{self, Line};
use crate::output;
use crate::transport::{join, Entry, Transport};
use crate::util::{now_micros, parse_time};

use super::fsops::{self, meta_dir, PeerRef};
use super::peer::{self, note_failure};

pub fn run(cfg: &Config, peers: &[PeerRef], ts: Option<i64>) -> i32 {
    if peers.is_empty() {
        output::error("no peer is reachable");
        return 1;
    }
    for p in peers {
        let target = match ts {
            Some(t) => t,
            None => match peer::last_run_start(p.transport.as_ref()).or_else(|| newest_bak(p.transport.as_ref(), "").map(|t| t - 1)) {
                Some(t) => t,
                None => {
                    output::line(&format!("nothing to undo for {}", p.url));
                    continue;
                }
            },
        };
        let r = Rollback { cfg, peer: p, t: target };
        r.dir("");
    }
    super::finish("rollback complete")
}

/// Newest evidence of a run anywhere under `dir`: a BAK timestamp or a
/// manifest `placed` value.
fn newest_bak(t: &dyn Transport, dir: &str) -> Option<i64> {
    let mut newest = None;
    if let Ok(entries) = t.list_dir(&join(&meta_dir(dir), "BAK")) {
        for e in entries {
            if let Some(ts) = parse_time(&e.name) {
                newest = newest.max(Some(ts));
            }
        }
    }
    if let Ok(Some(text)) = peer::read_manifest(t, dir) {
        for l in manifest::parse(&text).values() {
            newest = newest.max(l.placed);
        }
    }
    if let Ok(entries) = t.list_dir(dir) {
        for e in entries.iter().filter(|e| e.is_dir && e.name != ".kitchensync" && e.name != ".git") {
            newest = newest.max(newest_bak(t, &join(dir, &e.name)));
        }
    }
    newest
}

struct Rollback<'a> {
    cfg: &'a Config,
    peer: &'a PeerRef,
    t: i64,
}

impl Rollback<'_> {
    fn fail(&self, what: &str, rel: &str, e: &dyn std::fmt::Display) {
        output::error(&format!("rollback: {} failed for {} on {}: {}", what, rel, self.peer.url, e));
        note_failure();
    }

    fn dir(&self, dir: &str) {
        let t: &dyn Transport = self.peer.transport.as_ref();
        if !self.cfg.dry_run {
            if let Err(e) = peer::recover_manifest(t, dir).and_then(|_| fsops::recover_swaps(t, dir)) {
                self.fail("recovery", dir, &e);
                return;
            }
        }
        let live: Vec<Entry> = match t.list_dir(dir) {
            Ok(v) => v.into_iter().filter(|e| e.name != ".kitchensync" && e.name != ".git").collect(),
            Err(e) => {
                self.fail("listing", dir, &e);
                return;
            }
        };
        let mut lines: BTreeMap<String, Line> = match peer::read_manifest(t, dir) {
            Ok(Some(text)) => manifest::parse(&text),
            Ok(None) => BTreeMap::new(),
            Err(e) => {
                self.fail("manifest read", dir, &e);
                return;
            }
        };

        // Earliest BAK copy after T of every name (state at T).
        let bak_root = join(&meta_dir(dir), "BAK");
        let mut earliest: BTreeMap<String, (i64, String)> = BTreeMap::new();
        if let Ok(stamps) = t.list_dir(&bak_root) {
            let mut stamps: Vec<(i64, String)> = stamps.iter().filter_map(|e| parse_time(&e.name).map(|ts| (ts, e.name.clone()))).filter(|(ts, _)| *ts > self.t).collect();
            stamps.sort();
            for (ts, name) in stamps {
                if let Ok(items) = t.list_dir(&join(&bak_root, &name)) {
                    for it in items {
                        earliest.entry(it.name).or_insert((ts, name.clone()));
                    }
                }
            }
        }

        let archived_manifest = earliest.remove(manifest::NAME);
        let mut changed = false;

        // Restore entries that were replaced or removed after T.
        for (name, (_, stamp)) in &earliest {
            let rel = join(dir, name);
            let from = join(&join(&bak_root, stamp), name);
            output::info(&format!("R {rel}"));
            if self.cfg.dry_run {
                continue;
            }
            if live.iter().any(|e| &e.name == name) && !fsops::displace(self.peer, &rel, false) {
                continue;
            }
            if let Err(e) = t.rename(&from, &rel) {
                self.fail("restore", &rel, &e);
                continue;
            }
            let _ = t.delete_dir(&join(&bak_root, stamp));
            let is_dir = matches!(t.stat(&rel), Ok(s) if s.is_dir);
            let now = now_micros();
            lines.insert(name.clone(), Line { is_dir, mod_time: now, byte_size: if is_dir { -1 } else { 0 }, last_seen: Some(now), deleted_time: None, placed: None });
            changed = true;
        }

        // Remove entries KitchenSync placed after T.
        for e in live.iter().filter(|e| !earliest.contains_key(&e.name)) {
            let placed_after = lines.get(&e.name).and_then(|l| l.placed).is_some_and(|p| p > self.t);
            if placed_after {
                let rel = join(dir, &e.name);
                output::info(&format!("X {rel}"));
                if !self.cfg.dry_run && fsops::displace(self.peer, &rel, false) {
                    lines.remove(&e.name);
                    changed = true;
                }
            }
        }

        // Recurse into every directory that is live now.
        if let Ok(now_live) = t.list_dir(dir) {
            for e in now_live.iter().filter(|e| e.is_dir && e.name != ".kitchensync" && e.name != ".git") {
                self.dir(&join(dir, &e.name));
            }
        }

        if self.cfg.dry_run {
            return;
        }
        // Put the manifest back: the earliest archived one after T, else the
        // live one minus removed entries.
        if let Some((_, stamp)) = archived_manifest {
            let from = join(&join(&bak_root, &stamp), manifest::NAME);
            let text = match read_file(t, &from) {
                Ok(s) => s,
                Err(e) => {
                    self.fail("manifest restore", dir, &e);
                    return;
                }
            };
            if let Err(e) = peer::write_manifest(t, dir, &text) {
                self.fail("manifest write", dir, &e);
            }
            let _ = t.delete_file(&from);
            let _ = t.delete_dir(&join(&bak_root, &stamp));
        } else if changed {
            let cutoff = now_micros() - (self.cfg.keep_del_days as i64) * 86_400 * 1_000_000;
            if let Err(e) = peer::write_manifest(t, dir, &manifest::serialize(&lines, cutoff)) {
                self.fail("manifest write", dir, &e);
            }
        }
    }
}

fn read_file(t: &dyn Transport, path: &str) -> crate::transport::Result<String> {
    let mut r = t.open_read(path)?;
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = r.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}
