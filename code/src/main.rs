mod cleanup;
mod config;
mod database;
mod decision;
mod hash;
mod ignore;
mod lifecycle;
mod local_peer;
mod peer;
mod sftp_peer;
mod sync;
mod timestamp;
mod worker;

use std::collections::HashMap;
use std::sync::Arc;

const HELP_TEXT: &str = include_str!("help.txt");

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Parse CLI
    let mut config_arg: Option<&str> = None;
    let mut canon_peer: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print!("{}", HELP_TEXT);
                std::process::exit(0);
            }
            "--canon" => {
                i += 1;
                if i >= args.len() {
                    println!("--canon requires a peer name");
                    std::process::exit(1);
                }
                canon_peer = Some(args[i].clone());
            }
            arg => {
                if config_arg.is_some() {
                    println!("Unexpected argument: {}", arg);
                    std::process::exit(1);
                }
                config_arg = Some(arg);
            }
        }
        i += 1;
    }

    let config_arg = match config_arg {
        Some(a) => a,
        None => {
            print!("{}", HELP_TEXT);
            std::process::exit(0);
        }
    };

    // Resolve config file
    let config_path = match config::resolve_config_path(config_arg) {
        Ok(p) => p,
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    };

    // Load config
    let cfg = match config::load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    };

    // Validate --canon peer exists in config
    if let Some(ref canon) = canon_peer {
        if !cfg.peers.contains_key(canon) {
            println!("Unknown peer in --canon: {}", canon);
            std::process::exit(1);
        }
    }

    // Open database
    let conn = match database::open(&cfg.database_path) {
        Ok(c) => c,
        Err(e) => {
            println!("{}", e);
            std::process::exit(1);
        }
    };

    // Instance check
    if let Err(e) = lifecycle::instance_check(&conn, &config_path) {
        println!("{}", e);
        std::process::exit(1);
    }

    // Start HTTP server for instance management
    let conn_arc = Arc::new(std::sync::Mutex::new(conn));
    if let Err(e) = lifecycle::start_server(conn_arc.clone(), &config_path, cfg.log_retention_days)
    {
        println!("{}", e);
        std::process::exit(1);
    }

    // Connect to all peers in parallel
    let mut connected_peers: HashMap<String, Box<dyn peer::Peer>> = HashMap::new();
    let mut unreachable: Vec<String> = Vec::new();

    for (name, peer_cfg) in &cfg.peers {
        match peer::connect_peer(name, &peer_cfg.urls, cfg.connection_timeout) {
            Some(p) => {
                connected_peers.insert(name.clone(), p);
            }
            None => {
                unreachable.push(name.clone());
                eprintln!("WARNING: Peer {} unreachable, skipping", name);
                let conn = conn_arc.lock().unwrap();
                database::log(
                    &conn,
                    "warning",
                    &format!("Peer {} unreachable, skipping", name),
                    cfg.log_retention_days,
                );
            }
        }
    }

    // Canon peer must be reachable
    if let Some(ref canon) = canon_peer {
        if !connected_peers.contains_key(canon) {
            println!("Canon peer {} is unreachable", canon);
            std::process::exit(1);
        }
    }

    // Need at least two reachable peers (or one with --canon)
    if canon_peer.is_some() {
        if connected_peers.is_empty() {
            println!("No reachable peers");
            std::process::exit(1);
        }
    } else if connected_peers.len() < 2 {
        println!(
            "Need at least 2 reachable peers, found {}",
            connected_peers.len()
        );
        std::process::exit(1);
    }

    let peers = Arc::new(connected_peers);

    // Purge expired data
    {
        let conn = conn_arc.lock().unwrap();
        cleanup::purge_all(
            &conn,
            &peers,
            cfg.xfer_cleanup_days,
            cfg.back_retention_days,
            cfg.tombstone_retention_days,
            cfg.log_retention_days,
        );
    }

    // Start worker threads
    let pool = worker::WorkerPool::new(cfg.workers, peers.clone());

    // Run multi-tree sync
    {
        let conn = conn_arc.lock().unwrap();
        sync::run_sync(&conn, &peers, &pool, canon_peer.as_deref());
    }

    // Wait for copies to complete
    pool.wait();

    // Log completion
    {
        let conn = conn_arc.lock().unwrap();
        database::log(
            &conn,
            "info",
            "Sync complete",
            cfg.log_retention_days,
        );
    }

    // Sync complete — linger for 5 seconds serving HTTP, then exit
    std::thread::sleep(std::time::Duration::from_secs(5));
    std::process::exit(0);
}
