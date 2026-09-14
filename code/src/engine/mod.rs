//! Startup, run, and shutdown. See specs/sync.md ("Startup", "Run", "Rollback").

mod copy;
mod fsops;
mod peer;
mod rollback;
mod walk;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::config::{Config, Mode, PeerSpec, Role, Scheme};
use crate::output;
use crate::transport::{Transport, TransportError};

use copy::CopyQueue;
use fsops::PeerRef;
use peer::{Peer, FAILURES};

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

    let mut peers: Vec<PeerRef> = Vec::new();
    for (index, (spec, conn)) in cfg.peers.iter().zip(connected).enumerate() {
        let Some((url, transport)) = conn else { continue };
        if create_roots {
            if let Err(e) = peer::recover_manifest(transport.as_ref(), "") {
                output::error(&format!("peer unreachable: {}: {}", url, e));
                continue;
            }
        }
        let had_history = match transport.stat(&peer::manifest_path("")) {
            Ok(_) => true,
            Err(e) if e.is_not_found() => false,
            Err(e) => {
                output::error(&format!("peer unreachable: {}: {}", url, e));
                continue;
            }
        };
        peers.push(Arc::new(Peer { index, role: spec.role, url, transport, had_history }));
    }

    match cfg.mode {
        Mode::Sync => run_sync(cfg, peers),
        Mode::Rollback(ts) => rollback::run(cfg, &peers, Some(ts)),
        Mode::Undo => rollback::run(cfg, &peers, None),
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

    let run_start = crate::util::now_string();
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
    let queue = CopyQueue::new(cfg.parallel, cfg.retries_copy);
    let workers = queue.start_workers();
    let walker = walk::Walker { cfg: cfg.clone(), queue: Arc::clone(&queue), ignore: build_ignore(cfg, &peers), prefetch: walk::Prefetch::new() };
    std::thread::scope(|s| {
        for _ in 0..walk::PREFETCH_THREADS {
            s.spawn(|| walker.prefetch_worker());
        }
        walker.sync_directory(&peers, "");
        walker.prefetch.stop();
    });
    queue.close_and_wait(workers);

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
            Ok(t) => return Some((url.normalized.clone(), t)),
            Err(e) => output::error(&format!("peer unreachable: {}: {}", url.normalized, e)),
        }
    }
    None
}
