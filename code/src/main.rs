mod config;
mod connection_pool;
mod database;
mod filesystem;
mod hash;
mod local_fs;
mod server;
mod sftp_fs;
mod sync_engine;
mod timestamp;
mod url_normalize;

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Notify;

const HELP_TEXT: &str = include_str!("help.txt");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = config::parse_args(&args);

    if cli.help {
        print!("{}", HELP_TEXT);
        std::process::exit(0);
    }

    let rt = tokio::runtime::Runtime::new().expect("cannot create tokio runtime");
    let exit_code = rt.block_on(async_main(cli));
    std::process::exit(exit_code);
}

async fn async_main(cli: config::CliArgs) -> i32 {
    // 1. Resolve config directory
    let config_dir = config::resolve_config_dir(cli.cfg_path.as_deref());
    if let Err(e) = std::fs::create_dir_all(&config_dir) {
        println!("cannot create config directory: {}", e);
        return 1;
    }

    let config_file = config_dir.join("kitchensync-conf.json");
    let config_file_abs = std::fs::canonicalize(&config_file)
        .unwrap_or_else(|_| config_file.clone());
    println!("config: {}", config_file_abs.display());
    let db_path = config_dir.join("kitchensync.db");

    // 2. Load config, merge CLI
    let mut cfg = match config::load_config(&config_file) {
        Ok(c) => c,
        Err(e) => {
            println!("{}", e);
            return 1;
        }
    };

    config::merge_cli_settings(&mut cfg, &cli.settings);

    let group_idx = match config::merge_cli_urls(&mut cfg, &cli.urls) {
        Ok(idx) => idx,
        Err(e) => {
            println!("{}", e);
            return 1;
        }
    };

    // 3. Open database
    let db = match database::Database::open(&db_path) {
        Ok(d) => d,
        Err(e) => {
            println!("{}", e);
            return 1;
        }
    };

    // Initialize log-level in config table if absent
    if db.get_config("log-level").is_none() {
        let _ = db.set_config("log-level", cfg.log_level());
    }

    // 4. Instance check
    if server::check_existing_instance(&db, &config_dir).await {
        println!("Already running");
        return 0;
    }

    // 5. Reconcile peers
    let peer_map = match db.reconcile_peers(&cfg) {
        Ok(m) => m,
        Err(e) => {
            println!("{}", e);
            return 1;
        }
    };

    // Write merged config file
    if let Err(e) = config::save_config(&config_file, &cfg) {
        println!("{}", e);
        return 1;
    }

    let group = &cfg.peer_groups[group_idx];

    // 6. Group must have at least two peers
    if group.peers.len() < 2 {
        println!("Group '{}' has fewer than 2 peers", group.name);
        return 1;
    }

    // Check for canon peer among CLI URLs
    let mut runtime_canon: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    for (url, is_canon) in &cli.urls {
        if *is_canon {
            let norm = url_normalize::normalize_url(url).unwrap_or_default();
            runtime_canon.insert(norm, true);
        }
    }

    // Determine which peer is canon at runtime
    let has_any_canon = group.peers.iter().any(|p| p.canon)
        || group.peers.iter().any(|p| {
            p.urls.iter().any(|u| {
                let norm = url_normalize::normalize_url(u.url_str()).unwrap_or_default();
                runtime_canon.contains_key(&norm)
            })
        });

    // 7. Check if any peer has snapshot data
    let peer_ids: Vec<i64> = group
        .peers
        .iter()
        .enumerate()
        .filter_map(|(pi, _)| peer_map.get(&(group_idx, pi)).copied())
        .collect();

    if !db.any_peer_has_snapshots(&peer_ids) && !has_any_canon {
        println!("No snapshot history and no canon peer. First sync? Mark the authoritative peer with a trailing !");
        return 1;
    }

    // Start HTTP server
    let shutdown_notify = Arc::new(Notify::new());
    let port = match server::start_server(
        config_dir.clone(),
        db_path.clone(),
        shutdown_notify.clone(),
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            println!("cannot start server: {}", e);
            return 1;
        }
    };

    let _ = db.set_config("serving-port", &port.to_string());
    db.log("info", "startup", cfg.log_level());

    // 8. Connect to all peers in parallel
    let mut connect_futures = Vec::new();
    for (pi, peer) in group.peers.iter().enumerate() {
        let peer_id = peer_map[&(group_idx, pi)];
        let is_runtime_canon = peer.urls.iter().any(|u| {
            let norm = url_normalize::normalize_url(u.url_str()).unwrap_or_default();
            runtime_canon.contains_key(&norm)
        });
        let peer = peer.clone();
        let cfg_clone = cfg.clone();
        connect_futures.push(tokio::spawn(async move {
            connection_pool::connect_peer(&peer, peer_id, &cfg_clone, is_runtime_canon).await
        }));
    }

    let mut connected_peers: Vec<Arc<connection_pool::ConnectedPeer>> = Vec::new();
    let mut canon_peer_unreachable = false;

    for (pi, fut) in connect_futures.into_iter().enumerate() {
        match fut.await {
            Ok(Ok(cp)) => {
                connected_peers.push(Arc::new(cp));
            }
            Ok(Err(e)) => {
                db.log("info", &format!("peer '{}' unreachable: {}", group.peers[pi].name, e), cfg.log_level());
                // Check if this was the canon peer
                let is_canon = group.peers[pi].canon
                    || group.peers[pi].urls.iter().any(|u| {
                        let norm = url_normalize::normalize_url(u.url_str()).unwrap_or_default();
                        runtime_canon.contains_key(&norm)
                    });
                if is_canon {
                    canon_peer_unreachable = true;
                }
            }
            Err(e) => {
                db.log("error", &format!("connect task failed: {}", e), cfg.log_level());
            }
        }
    }

    // 9. Canon peer must be reachable
    if canon_peer_unreachable {
        println!("Canon peer is unreachable");
        return 1;
    }

    if connected_peers.len() < 2 {
        // With a canon peer, one reachable is sufficient
        let has_canon = connected_peers.iter().any(|p| p.canon);
        if !has_canon || connected_peers.is_empty() {
            println!("Fewer than 2 peers reachable");
            return 1;
        }
    }

    // Run sync
    let sync_ts = timestamp::now();

    // Purge old data
    let _ = db.purge_tombstones(cfg.tombstone_retention_days());
    let _ = db.purge_old_logs(cfg.log_retention_days());

    // Run combined-tree walk
    if let Err(e) = sync_engine::run_sync(&connected_peers, &db, &cfg, &sync_ts).await {
        db.log("error", &format!("sync failed: {}", e), cfg.log_level());
    }

    db.log("info", "sync complete", cfg.log_level());

    // Post-completion linger: serve HTTP for 5 seconds, then exit
    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
        _ = shutdown_notify.notified() => {}
    }

    0
}
