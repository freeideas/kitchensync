use std::sync::Arc;
use crate::api::*;

struct RunControllerImpl {
    _cli: Arc<dyn cli::Cli>,
    transport: Arc<dyn transport::Transport>,
    snapshot: Arc<dyn snapshot::Snapshot>,
    syncengine: Arc<dyn syncengine::SyncEngine>,
    copyqueue: Arc<dyn copyqueue::CopyQueue>,
    output: Arc<dyn output::Output>,
}

impl RunController for RunControllerImpl {
    fn run(&self, config: cli::RunConfig) -> RunOutcome {
        let dry_run = config.options.dry_run;
        let timeout_conn =
            std::time::Duration::from_secs(config.options.timeout_conn as u64);

        self.output.set_verbosity(match &config.options.verbosity {
            cli::Verbosity::Error => output::Verbosity::Error,
            cli::Verbosity::Info => output::Verbosity::Info,
            cli::Verbosity::Debug => output::Verbosity::Debug,
            cli::Verbosity::Trace => output::Verbosity::Trace,
        });

        if dry_run {
            self.output.diagnostic("dry run: no peer changes will be applied.");
        }

        // Pre-verify canon peers exist before open_peer can create missing directories.
        // In non-dry-run mode, open_peer calls create_dir_all on local paths, which
        // would silently create a non-existent canon source. Check with dry_run=true
        // first so a missing canon peer produces a clean exit-1 (022.17).
        if !dry_run {
            for peer in &config.peers {
                if matches!(peer.role, cli::PeerRole::Canon) {
                    let primary = peer.urls[0].url.clone();
                    let fallbacks: Vec<String> =
                        peer.urls[1..].iter().map(|u| u.url.clone()).collect();
                    if self.transport.open_peer(&primary, &fallbacks, true, timeout_conn).is_none() {
                        return RunOutcome { exit_code: 1, message: None };
                    }
                }
            }
        }

        // Start connection attempts to all peers concurrently (006.1).
        let peer_connections: Vec<Option<transport::ConnectedPeer>> = {
            let handles: Vec<std::thread::JoinHandle<Option<transport::ConnectedPeer>>> =
                config.peers.iter().map(|peer| {
                    let t = Arc::clone(&self.transport);
                    let primary = peer.urls[0].url.clone();
                    let fallbacks: Vec<String> =
                        peer.urls[1..].iter().map(|u| u.url.clone()).collect();
                    std::thread::spawn(move || {
                        t.open_peer(&primary, &fallbacks, dry_run, timeout_conn)
                    })
                }).collect();
            handles.into_iter().map(|h| h.join().unwrap_or(None)).collect()
        };

        // Gate: fewer than two peers reachable (006.2).
        if peer_connections.iter().filter(|c| c.is_some()).count() < 2 {
            return RunOutcome { exit_code: 1, message: None };
        }

        // Gate: designated canon peer is unreachable (006.3).
        let has_canon = config.peers.iter().any(|p| matches!(p.role, cli::PeerRole::Canon));
        if has_canon {
            let canon_reachable = config.peers.iter().zip(peer_connections.iter())
                .any(|(p, c)| matches!(p.role, cli::PeerRole::Canon) && c.is_some());
            if !canon_reachable {
                return RunOutcome { exit_code: 1, message: None };
            }
        }

        // Probe each reachable peer for an existing snapshot database.
        let has_snapshot: Vec<bool> = peer_connections.iter()
            .map(|conn| {
                conn.as_ref().map_or(false, |cp| {
                    self.transport.stat(&cp.handle, ".kitchensync/snapshot.db").is_ok()
                })
            })
            .collect();

        // Gate: no reachable peer has snapshot data and no canon designated (006.4, 006.5).
        if !has_canon {
            let any_has_snapshot = peer_connections.iter().zip(has_snapshot.iter())
                .any(|(c, hs)| c.is_some() && *hs);
            if !any_has_snapshot {
                return RunOutcome {
                    exit_code: 1,
                    message: Some(
                        "First sync? Mark the authoritative peer with a leading +".to_string(),
                    ),
                };
            }
        }

        // Gate: no contributing peer after auto-subordination of snapshotless peers (006.6, 006.7).
        // Canon is always non-subordinate; a Normal peer with no snapshot is auto-subordinated.
        let any_contributing = config.peers.iter()
            .zip(peer_connections.iter())
            .zip(has_snapshot.iter())
            .any(|((peer, conn), hs)| {
                conn.is_some()
                    && match &peer.role {
                        cli::PeerRole::Canon => true,
                        cli::PeerRole::Normal => *hs,
                        cli::PeerRole::Subordinate => false,
                    }
            });
        if !any_contributing {
            return RunOutcome {
                exit_code: 1,
                message: Some(
                    "No contributing peer reachable - cannot make sync decisions".to_string(),
                ),
            };
        }

        // Recover any interrupted SWAP and download each reachable peer's snapshot.
        for conn in peer_connections.iter() {
            if let Some(cp) = conn {
                if let Err(e) = self.snapshot.open(&cp.winning_url, dry_run) {
                    let msg = match e {
                        snapshot::SnapshotError::Database(s) => {
                            format!("snapshot error for {}: {}", cp.winning_url, s)
                        }
                        snapshot::SnapshotError::Transport(s) => {
                            format!("snapshot transport error for {}: {}", cp.winning_url, s)
                        }
                    };
                    self.output.diagnostic(&msg);
                }
            }
        }

        // Configure the copy queue before any enqueue or parallel-executor call.
        self.copyqueue.configure(copyqueue::CopyConfig {
            copy_slot_limit: Some(config.options.max_copies as usize),
            copy_try_limit: Some(config.options.retries_copy),
            bak_retention: Some(std::time::Duration::from_secs(
                config.options.keep_bak_days as u64 * 86_400,
            )),
            tmp_retention: Some(std::time::Duration::from_secs(
                config.options.keep_tmp_days as u64 * 86_400,
            )),
            dry_run,
        });

        // Build the reachable-peer list for the sync engine.
        let sync_peers: Vec<syncengine::SyncPeer> = config.peers.iter()
            .zip(peer_connections.iter())
            .filter_map(|(peer, conn)| {
                conn.as_ref().map(|cp| syncengine::SyncPeer {
                    url: cp.winning_url.clone(),
                    role: match &peer.role {
                        cli::PeerRole::Canon => syncengine::PeerRole::Canon,
                        cli::PeerRole::Subordinate => syncengine::PeerRole::Subordinate,
                        cli::PeerRole::Normal => syncengine::PeerRole::Contributing,
                    },
                    prefix: String::new(),
                })
            })
            .collect();

        // Drive the combined-tree traversal and interleaved copy phase.
        self.syncengine.run(syncengine::RunRequest {
            peers: sync_peers,
            excludes: config.excludes,
            list_retries: config.options.retries_list,
            dry_run,
        });

        // Wait for every enqueued copy to reach a terminal outcome (006.9).
        self.copyqueue.wait();

        // Opportunistic snapshot row cleanup after the traversal, before writeback
        // (018.1-018.3); the snapshot service owns the maintenance mechanics.
        for conn in peer_connections.iter() {
            if let Some(cp) = conn {
                let _ = self.snapshot.prune(&cp.winning_url, config.options.keep_del_days);
            }
        }

        // Write updated snapshots back to each reachable peer (006.10).
        for conn in peer_connections.iter() {
            if let Some(cp) = conn {
                if let Err(e) = self.snapshot.writeback(&cp.winning_url, dry_run) {
                    let msg = match e {
                        snapshot::SnapshotError::Database(s) => {
                            format!("snapshot writeback error for {}: {}", cp.winning_url, s)
                        }
                        snapshot::SnapshotError::Transport(s) => {
                            format!("snapshot writeback transport error for {}: {}", cp.winning_url, s)
                        }
                    };
                    self.output.diagnostic(&msg);
                }
            }
        }

        // All phases complete (006.11).
        RunOutcome { exit_code: 0, message: None }
    }
}

pub fn new(
    cli: Arc<dyn cli::Cli>,
    transport: Arc<dyn transport::Transport>,
    snapshot: Arc<dyn snapshot::Snapshot>,
    syncengine: Arc<dyn syncengine::SyncEngine>,
    copyqueue: Arc<dyn copyqueue::CopyQueue>,
    output: Arc<dyn output::Output>,
) -> Arc<dyn RunController> {
    Arc::new(RunControllerImpl { _cli: cli, transport, snapshot, syncengine, copyqueue, output })
}
