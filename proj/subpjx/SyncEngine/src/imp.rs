use std::sync::{Arc, Mutex};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use crate::api::*;

// ── path helpers ──────────────────────────────────────────────────────────────

fn join(parent: &str, child: &str) -> String {
    match (parent.is_empty(), child.is_empty()) {
        (true, _) => child.to_string(),
        (_, true) => parent.to_string(),
        _ => format!("{}/{}", parent, child),
    }
}

fn is_builtin_excluded(name: &str) -> bool {
    name == ".kitchensync" || name == ".git"
}

// ── timestamp conversion ──────────────────────────────────────────────────────

fn to_ts(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let s = d.as_secs();
    let us = d.subsec_micros();
    let hh = ((s % 86400) / 3600) as u32;
    let mm = ((s % 3600) / 60) as u32;
    let ss = (s % 60) as u32;
    let (y, mo, day) = civil_from_days(s / 86400);
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}_{:06}Z", y, mo, day, hh, mm, ss, us)
}

// Howard Hinnant's civil_from_days algorithm
fn civil_from_days(days: u64) -> (u32, u32, u32) {
    let z = days as i64 + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if mo <= 2 { y + 1 } else { y } as u32;
    (y, mo, d)
}

// ── transport helpers ─────────────────────────────────────────────────────────

fn retry_list(
    t: &dyn transport::Transport,
    h: &transport::PeerHandle,
    path: &str,
    tries: u32,
) -> Option<Vec<transport::DirEntry>> {
    for _ in 0..tries.max(1) {
        if let Ok(v) = t.list_dir(h, path) {
            return Some(v);
        }
    }
    None
}

// ── role conversion ───────────────────────────────────────────────────────────

fn to_dr_role(r: &EffRole) -> syncengine_decisionrules::PeerRole {
    match r {
        EffRole::Canon => syncengine_decisionrules::PeerRole::Canon,
        EffRole::Contributing => syncengine_decisionrules::PeerRole::Contributing,
        EffRole::Subordinate => syncengine_decisionrules::PeerRole::Subordinate,
    }
}

// ── internal types ────────────────────────────────────────────────────────────

#[derive(PartialEq, Eq)]
enum EffRole {
    Canon,
    Contributing,
    Subordinate,
}

struct Peer {
    url: String,
    role: EffRole,
    prefix: String,
    handle: transport::PeerHandle,
}

// ── implementation ────────────────────────────────────────────────────────────

struct SyncEngineImpl {
    copyqueue: Arc<dyn copyqueue::CopyQueue>,
    output: Arc<dyn output::Output>,
    snapshot: Arc<dyn snapshot::Snapshot>,
    transport: Arc<dyn transport::Transport>,
    decision_rules: Arc<dyn syncengine_decisionrules::DecisionRules>,
    displacement: Arc<dyn syncengine_displacement::Displacement>,
}

impl SyncEngine for SyncEngineImpl {
    fn run(&self, request: RunRequest) {
        self.copyqueue.configure(copyqueue::CopyConfig {
            copy_slot_limit: None,
            copy_try_limit: None,
            bak_retention: None,
            tmp_retention: None,
            dry_run: request.dry_run,
        });

        let mut peers: Vec<Peer> = Vec::new();
        for sp in &request.peers {
            let c = match self.transport.open_peer(
                &sp.url, &[], request.dry_run, Duration::from_secs(30),
            ) {
                Some(c) => c,
                None => continue,
            };
            // The snapshot working copy is opened (downloaded) by the run controller
            // before the walk; the snapshot service's download/upload lifecycle and
            // its row maintenance are owned by RunController and Snapshot, not here.
            // Peers with no snapshot.db are treated as subordinate unless canon (007.7-007.9)
            let db_path = join(&sp.prefix, ".kitchensync/snapshot.db");
            let has_db = self.transport.stat(&c.handle, &db_path).is_ok();
            let role = match &sp.role {
                PeerRole::Canon => EffRole::Canon,
                PeerRole::Subordinate => EffRole::Subordinate,
                PeerRole::Contributing => {
                    if has_db { EffRole::Contributing } else { EffRole::Subordinate }
                }
            };
            peers.push(Peer {
                url: c.winning_url,
                role,
                prefix: sp.prefix.clone(),
                handle: c.handle,
            });
        }

        let all: Vec<usize> = (0..peers.len()).collect();
        self.walk(&peers, &all, "", &request);

        // Drain all enqueued copies before returning (006.9).
        self.copyqueue.wait();

        // Snapshot row maintenance (018) and writeback (006.10) are driven by the
        // run controller after the walk returns; SyncEngine only records the rows.
    }
}

impl SyncEngineImpl {
    fn walk(&self, peers: &[Peer], active: &[usize], rel: &str, req: &RunRequest) {
        if active.is_empty() {
            return;
        }

        // SWAP recovery before listing; failure excludes peer from this subtree
        let mut swap_ok: Vec<usize> = Vec::new();
        for &i in active {
            let p = &peers[i];
            if self.copyqueue.recover_swap(&p.url, &join(&p.prefix, rel)) {
                swap_ok.push(i);
            }
        }

        // List each peer's directory in parallel using the shared executor (008.3)
        type ListResult = Option<Vec<transport::DirEntry>>;
        let result_cells: Vec<Mutex<ListResult>> =
            (0..swap_ok.len()).map(|_| Mutex::new(None)).collect();

        let jobs: Vec<Box<dyn FnOnce() + Send + '_>> = swap_ok
            .iter()
            .zip(result_cells.iter())
            .map(|(&i, cell)| {
                let p = &peers[i];
                let path = join(&p.prefix, rel);
                let tries = req.list_retries;
                let t = self.transport.as_ref();
                let h = &p.handle;
                Box::new(move || {
                    *cell.lock().unwrap() = retry_list(t, h, &path, tries);
                }) as Box<dyn FnOnce() + Send + '_>
            })
            .collect();
        self.copyqueue.run_in_parallel(jobs);

        let lmap: HashMap<usize, ListResult> = swap_ok
            .iter()
            .zip(result_cells.into_iter())
            .map(|(&i, cell)| (i, cell.into_inner().unwrap()))
            .collect();

        // Peers with successful listings drive decisions
        let ok: Vec<usize> = swap_ok.iter().copied()
            .filter(|i| lmap.get(i).and_then(|l| l.as_ref()).is_some())
            .collect();

        // Emit diagnostic for each peer excluded from this directory (008.10)
        let ok_set: HashSet<usize> = ok.iter().copied().collect();
        for &i in active {
            if !ok_set.contains(&i) {
                let p = &peers[i];
                let dir = join(&p.prefix, rel);
                let loc = if dir.is_empty() { p.url.as_str() } else { dir.as_str() };
                self.output.diagnostic(&format!(
                    "listing failed for {} on {}", loc, p.url
                ));
            }
        }

        // Canon listing failure: no peer modified under this subtree (008.13-008.14)
        let canon_failed = active.iter().any(|&i| {
            peers[i].role == EffRole::Canon && !ok_set.contains(&i)
        });
        if canon_failed {
            return;
        }

        // All contributing peers failed listing: skip subtree (008.15)
        let has_contributing = active.iter().any(|&i| {
            matches!(peers[i].role, EffRole::Canon | EffRole::Contributing)
        });
        let any_contributing_ok = ok.iter().any(|&i| {
            matches!(peers[i].role, EffRole::Canon | EffRole::Contributing)
        });
        if has_contributing && !any_contributing_ok {
            return;
        }

        // Build name union from all successfully-listed peers; sort case-insensitively
        let mut names: HashSet<String> = HashSet::new();
        for &i in &ok {
            for e in lmap[&i].as_ref().unwrap() {
                if is_builtin_excluded(&e.name) {
                    continue;
                }
                if req.excludes.iter().any(|x| x == &join(rel, &e.name)) {
                    continue;
                }
                names.insert(e.name.clone());
            }
        }
        let mut sorted: Vec<String> = names.into_iter().collect();
        sorted.sort_by(|a, b| {
            a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
        });

        // Process every entry; finish all before recursing (008.1, 008.2)
        let mut recurse: Vec<(String, Vec<usize>)> = Vec::new();
        for name in &sorted {
            if let Some(sub) = self.process(peers, &ok, &lmap, rel, name, req) {
                recurse.push(sub);
            }
        }

        // BAK/TMP cleanup after the union is processed; skip under dry_run (021.9)
        if !req.dry_run {
            for &i in &ok {
                self.copyqueue.cleanup(&peers[i].url, &join(&peers[i].prefix, rel));
            }
        }

        // Recurse into kept directories
        for (nm, ri) in recurse {
            self.walk(peers, &ri, &join(rel, &nm), req);
        }
    }

    fn process(
        &self,
        peers: &[Peer],
        ok: &[usize],
        lmap: &HashMap<usize, Option<Vec<transport::DirEntry>>>,
        rel: &str,
        name: &str,
        req: &RunRequest,
    ) -> Option<(String, Vec<usize>)> {
        struct Li {
            is_dir: bool,
            st: Option<SystemTime>,
            bs: Option<i64>,
        }

        let er = join(rel, name);
        let id = self.snapshot.path_identity(&er);
        let pid = if rel.is_empty() {
            self.snapshot.path_identity("/")
        } else {
            self.snapshot.path_identity(rel)
        };

        // Gather per-peer live state and build decision inputs
        let mut lis: Vec<Li> = Vec::new();
        let mut inputs: Vec<syncengine_decisionrules::PeerInput> = Vec::new();
        for &i in ok {
            let p = &peers[i];
            let found = lmap[&i].as_ref().unwrap().iter().find(|e| e.name == name);
            let (live_dr, li) = match found {
                Some(e) if e.is_dir => (
                    syncengine_decisionrules::LiveEntry::Directory,
                    Li { is_dir: true, st: Some(e.mod_time), bs: None },
                ),
                Some(e) => (
                    syncengine_decisionrules::LiveEntry::File {
                        byte_size: e.byte_size,
                        mod_time: to_ts(e.mod_time),
                    },
                    Li { is_dir: false, st: Some(e.mod_time), bs: Some(e.byte_size) },
                ),
                None => (
                    syncengine_decisionrules::LiveEntry::Absent,
                    Li { is_dir: false, st: None, bs: None },
                ),
            };
            let row = self.snapshot.read_row(&p.url, &id).ok().flatten().map(|r| {
                syncengine_decisionrules::PeerRow {
                    byte_size: r.byte_size,
                    mod_time: r.mod_time,
                    deleted_time: r.deleted_time,
                    last_seen: r.last_seen,
                }
            });
            lis.push(li);
            inputs.push(syncengine_decisionrules::PeerInput {
                peer: p.url.clone(),
                role: to_dr_role(&p.role),
                live: live_dr,
                row,
            });
        }

        let dec = self.decision_rules.decide(&inputs);

        // Snapshot: confirm present or absent for each active peer.
        // Skip displaced peers so record_displaced can read pre-run last_seen (017.15).
        for (k, &i) in ok.iter().enumerate() {
            if dec.actions[k].displace {
                continue;
            }
            let p = &peers[i];
            if let Some(st) = lis[k].st {
                let bs = if lis[k].is_dir { -1i64 } else { lis[k].bs.unwrap_or(-1) };
                let _ = self.snapshot.record_present(&p.url, &id, &pid, name, &to_ts(st), bs);
            } else if self.snapshot.read_row(&p.url, &id).ok().flatten().is_some() {
                let _ = self.snapshot.record_absent(&p.url, &id);
            }
        }

        // Winner info for CopyWinner actions
        let winner: Option<(usize, SystemTime, i64)> = dec.winner.as_ref().and_then(|wu| {
            ok.iter().enumerate()
                .find(|(_, &i)| peers[i].url == *wu)
                .and_then(|(k, &i)| lis[k].st.map(|t| (i, t, lis[k].bs.unwrap_or(-1))))
        });

        // Execute per-peer actions
        let mut displaced: HashSet<usize> = HashSet::new();
        let mut created: HashSet<usize> = HashSet::new();
        let mut any_displaced = false;
        let mut any_copy = false;
        let mut any_dir_created = false;

        for (k, outcome) in dec.actions.iter().enumerate() {
            let pi = ok[k];
            let p = &peers[pi];

            // Displacement runs inline before any conform step (008.6)
            if outcome.displace {
                let ts = self.snapshot.now();
                let res = self.displacement.displace(
                    self.transport.as_ref(),
                    self.output.as_ref(),
                    &p.handle,
                    &join(&p.prefix, rel),
                    name,
                    &ts,
                    req.dry_run,
                );
                if matches!(res, syncengine_displacement::DisplaceOutcome::Displaced) {
                    let _ = self.snapshot.record_displaced(&p.url, &id);
                    displaced.insert(pi);
                    any_displaced = true;
                }
            }

            match &outcome.conform {
                syncengine_decisionrules::Conform::CopyWinner => {
                    if let Some((wi, wt, wbs)) = winner {
                        let wp = &peers[wi];
                        let _ = self.snapshot.record_push(
                            &p.url, &id, &pid, name, &to_ts(wt), wbs,
                        );
                        let snapshot = Arc::clone(&self.snapshot);
                        let dst_peer = p.url.clone();
                        let path_id = id.clone();
                        self.copyqueue.enqueue(copyqueue::CopyRequest {
                            src_peer: wp.url.clone(),
                            src_path: join(&wp.prefix, &er),
                            dst_peer: p.url.clone(),
                            dst_path: join(&p.prefix, &er),
                            mod_time: wt,
                            on_success: Some(Box::new(move || {
                                let _ = snapshot.record_copied(&dst_peer, &path_id);
                            })),
                        });
                        any_copy = true;
                    }
                }
                syncengine_decisionrules::Conform::CreateDirectory => {
                    let dir_full = join(&p.prefix, &er);
                    if !req.dry_run {
                        match self.transport.create_dir(&p.handle, &dir_full) {
                            Ok(()) => {
                                let _ = self.snapshot.record_present(
                                    &p.url, &id, &pid, name, &self.snapshot.now(), -1,
                                );
                                created.insert(pi);
                                any_dir_created = true;
                            }
                            Err(_) => {
                                self.output.diagnostic(&format!(
                                    "failed to create directory {} on {}", dir_full, p.url
                                ));
                            }
                        }
                    } else {
                        created.insert(pi);
                        any_dir_created = true;
                    }
                }
                syncengine_decisionrules::Conform::Nothing => {}
            }
        }

        if any_displaced {
            self.output.displaced(&er);
        }
        if any_copy {
            self.output.copied(&er);
        }

        // Collect peers to recurse into for kept directories
        if matches!(dec.agreed_type, syncengine_decisionrules::DecidedType::Directory) {
            let mut ri: Vec<usize> = Vec::new();
            for (k, _) in dec.actions.iter().enumerate() {
                let pi = ok[k];
                let had = lis[k].is_dir && lis[k].st.is_some() && !displaced.contains(&pi);
                let made = created.contains(&pi);
                if had || made {
                    ri.push(pi);
                }
            }
            if !ri.is_empty() {
                return Some((name.to_string(), ri));
            }
        }

        None
    }
}

pub fn new(
    copyqueue: Arc<dyn copyqueue::CopyQueue>,
    output: Arc<dyn output::Output>,
    snapshot: Arc<dyn snapshot::Snapshot>,
    transport: Arc<dyn transport::Transport>,
) -> std::sync::Arc<dyn SyncEngine> {
    // SyncEngine owns its decision-rules and displacement helpers and builds
    // them itself; they are an implementation detail and never appear in the
    // constructor's parameter list (SPEC: Construction and the hidden helpers).
    let decision_rules = syncengine_decisionrules::new();
    let displacement = syncengine_displacement::new();
    Arc::new(SyncEngineImpl {
        copyqueue,
        output,
        snapshot,
        transport,
        decision_rules,
        displacement,
    })
}
