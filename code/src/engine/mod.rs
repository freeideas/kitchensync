//! Startup, run, and shutdown. See specs/sync.md ("Startup", "Run", "Rollback").

mod copy;
mod fsops;
mod moves;
mod peer;
mod rollback;
mod walk;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::config::{Config, Mode, PeerSpec, Role, Scheme};
use crate::output;
use crate::state;
use crate::transport::{join, Transport, TransportError};

use copy::CopyQueue;
use fsops::PeerRef;
use peer::{meta, History, Peer, FAILURES};

const FIRST_SYNC_MSG: &str = "first sync: no history found, merging both ways (nothing will be deleted); use + to make one peer authoritative";
const NO_CONTRIBUTING_MSG: &str = "No contributing peer reachable - cannot make sync decisions";

/// Run a sync (or rollback) and return the process exit code.
pub fn run(cfg: Config) -> i32 {
    let cfg = &cfg;
    if cfg.dry_run {
        output::line("dry run");
    }
    // Startup 2: connect all peers in parallel.
    let create_roots = !cfg.dry_run && cfg.mode == Mode::Sync;
    let connected: Vec<Option<(String, Arc<dyn Transport>)>> = std::thread::scope(|s| {
        let handles: Vec<_> = cfg.peers.iter().map(|spec| s.spawn(move || connect_peer(cfg, spec, create_roots))).collect();
        handles.into_iter().map(|h| h.join().unwrap_or(None)).collect()
    });

    let run_start = crate::util::now_string();
    let mut peers: Vec<PeerRef> = Vec::new();
    for (index, (spec, conn)) in cfg.peers.iter().zip(connected).enumerate() {
        let Some((url, transport)) = conn else { continue };
        match open_peer(cfg, index, spec.role, url.clone(), transport, &run_start) {
            Ok(p) => peers.push(Arc::new(p)),
            Err(e) => output::error(&format!("peer unreachable: {}: {}", url, e)),
        }
    }

    match cfg.mode {
        Mode::Sync => run_sync(cfg, peers),
        Mode::Rollback(ts) => rollback::run(cfg, &peers, Some(ts)),
        Mode::Undo => rollback::run(cfg, &peers, None),
    }
}

/// Startup 5 for one peer: repair, then read its history into memory
/// (specs/sync.md, "Startup"; specs/state.md, "Reading").
fn open_peer(cfg: &Config, index: usize, role: Role, url: String, transport: Arc<dyn Transport>, run: &str) -> Result<Peer, TransportError> {
    let t = transport.as_ref();
    if !cfg.dry_run {
        peer::recover_meta_file(t, state::STATE)?;
        peer::recover_meta_file(t, peer::RUNS)?;
    }
    // A dry run repairs nothing, so it reads what a repair would keep.
    let names: &[&str] = if cfg.dry_run { &["state.txt", "state.txt.new", "state.txt.old"] } else { &["state.txt"] };
    let mut text = None;
    for n in names {
        text = peer::read_text(t, &meta(n))?;
        if text.is_some() {
            break;
        }
    }
    let manifest = match text {
        Some(_) => None,
        None => peer::read_text(t, &meta(state::LEGACY_MANIFEST))?,
    };
    let history = History::new(text.as_deref(), cfg.keep_del_days);
    if let Some(m) = &manifest {
        history.settle("", state::parse_manifest(m));
    }
    let had_history = text.is_some() || manifest.is_some();
    let p = Peer::new(index, role, url, transport, had_history, run.to_string(), cfg.dry_run, cfg.keep_del_days, history);
    if !cfg.dry_run && cfg.mode == Mode::Sync {
        fsops::cleanup_root(&p, cfg.keep_bak_days);
    }
    Ok(p)
}

/// Fill a peer's move index: its large live files, and the files its
/// journals say are in BAK (specs/sync.md, "Moved Files").
fn index_moves(p: &Peer) {
    for (path, size, mt) in p.history.all_files(moves::MOVE_MIN) {
        p.moves.add(path, size, mt);
    }
    let dir = meta("journal");
    let Ok(entries) = p.t().list_dir(&dir) else { return };
    let mut lines: Vec<state::JLine> = Vec::new();
    for e in entries.iter().filter(|e| !e.is_dir) {
        if let Ok(Some(text)) = peer::read_text(p.t(), &join(&dir, &e.name)) {
            lines.extend(state::parse_journal(&text));
        }
    }
    let moved_out: std::collections::HashSet<&str> = lines.iter().filter(|l| l.op == 'M').filter_map(|l| l.other.as_deref()).collect();
    for l in &lines {
        let (Some(other), Some(size), Some(mt)) = (&l.other, l.byte_size, l.mod_time) else { continue };
        if (l.op == 'X' || l.op == 'B') && !moved_out.contains(other.as_str()) {
            p.moves.add(other.clone(), size, mt);
        }
    }
}

fn run_sync(cfg: &Config, mut peers: Vec<PeerRef>) -> i32 {
    // Startup 3-4.
    if peers.len() < 2 {
        output::error("fewer than two peers are reachable");
        return 1;
    }
    let canon_wanted = cfg.peers.iter().any(|p| p.role == Role::Canon);
    if canon_wanted && !peers.iter().any(|p| p.is_canon()) {
        output::error("canon peer is unreachable");
        return 1;
    }

    // Startup 5-6: auto-subordinate peers joining an established group.
    let any_history = peers.iter().any(|p| p.had_history);
    if any_history {
        for p in peers.iter_mut() {
            if !p.had_history && p.role != Role::Canon && p.role != Role::Subordinate {
                let inner = Arc::get_mut(p).expect("peer not yet shared");
                inner.role = Role::Subordinate;
            }
        }
    } else if !canon_wanted {
        output::line(FIRST_SYNC_MSG);
    }
    if !peers.iter().any(|p| p.contributes()) {
        output::line(NO_CONTRIBUTING_MSG);
        return 1;
    }

    let run_start = peers[0].run.clone();
    let list: Vec<String> = peers.iter().map(|p| quote(&p.url)).collect();
    if !cfg.dry_run {
        output::info(&format!("undo later with: kitchensync --rollback {} {}", run_start, list.join(" ")));
        let line = format!("{}\t{}", run_start, list.join(" "));
        for p in &peers {
            if let Err(e) = peer::append_run(p.transport.as_ref(), &line) {
                output::error(&format!("run record failed for {}: {}", p.url, e));
            }
        }
    }

    // Run 1-3: walk and wait for copies.
    for p in &peers {
        index_moves(p);
    }
    let queue = CopyQueue::new(cfg.parallel, cfg.retries_copy, peers.clone());
    let workers = queue.start_workers();
    let walker = walk::Walker { cfg: cfg.clone(), queue: Arc::clone(&queue), ignore: build_ignore(cfg, &peers), prefetch: walk::Prefetch::new() };
    std::thread::scope(|s| {
        for _ in 0..walk::PREFETCH_THREADS {
            s.spawn(|| walker.prefetch_worker());
        }
        walker.sync_directory(&peers, "", &Default::default());
        walker.prefetch.stop();
    });
    queue.close_and_wait(workers);
    for p in &peers {
        p.write_state();
        p.close_journal();
        if !cfg.dry_run {
            // The root's own per-directory manifest is now in the state file.
            for n in ["manifest.txt", "manifest.txt.new", "manifest.txt.old"] {
                let _ = p.t().delete_file(&meta(n));
            }
        }
    }

    finish("sync complete")
}

/// Built-ins, then every peer's `.kitchensync/ignore`, then `-x` patterns.
fn build_ignore(cfg: &Config, peers: &[PeerRef]) -> crate::ignore::IgnoreSet {
    let mut set = crate::ignore::IgnoreSet::builtin();
    for p in peers {
        match peer::read_ignore(p.transport.as_ref()) {
            Ok(Some(text)) => {
                if let Err(e) = set.add_text(&text) {
                    output::error(&format!("ignore file on {} skipped: {}", p.url, e));
                }
            }
            Ok(None) => {}
            Err(e) => output::error(&format!("ignore file on {} unreadable: {}", p.url, e)),
        }
    }
    set.extend(&cfg.ignore);
    set
}

fn finish(label: &str) -> i32 {
    let failures = FAILURES.load(Ordering::SeqCst);
    if failures == 0 {
        output::line(label);
        0
    } else {
        output::line(&format!("{label} with {failures} failures"));
        2
    }
}

fn quote(s: &str) -> String {
    if s.contains(' ') {
        format!("\"{s}\"")
    } else {
        s.to_string()
    }
}

/// Try each URL in order; first connection wins.
fn connect_peer(cfg: &Config, spec: &PeerSpec, create: bool) -> Option<(String, Arc<dyn Transport>)> {
    for url in &spec.urls {
        let result: Result<Arc<dyn Transport>, TransportError> = match url.scheme {
            Scheme::File => crate::transport::local::LocalTransport::connect(&url.path, create).map(|t| Arc::new(t) as Arc<dyn Transport>),
            Scheme::Sftp => crate::transport::sftp::SftpTransport::connect(
                url,
                url.timeout_conn.unwrap_or(cfg.timeout_conn),
                url.timeout_idle.unwrap_or(cfg.timeout_idle),
                create,
            )
            .map(|t| Arc::new(t) as Arc<dyn Transport>),
        };
        match result {
            Ok(t) => return Some((url.normalized.clone(), Arc::new(crate::transport::normalize::Normalizing::new(t)))),
            Err(e) => output::error(&format!("peer unreachable: {}: {}", url.normalized, e)),
        }
    }
    None
}
