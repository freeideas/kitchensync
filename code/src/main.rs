mod timestamp;
mod path_hash;
mod database;
mod config;
mod ignore;
mod filesystem;
mod walker;
mod reconcile;
mod transfer;
mod connection;
mod watcher;
mod http_server;
mod cleanup;

use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

const HELP_TEXT: &str = include_str!("../../specs/help.txt");

fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse arguments
    let mut once_mode = false;
    let mut directory: Option<PathBuf> = None;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print!("{}", HELP_TEXT);
                return;
            }
            "--once" => {
                once_mode = true;
            }
            arg if !arg.starts_with('-') => {
                directory = Some(PathBuf::from(arg));
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    // Default to current directory
    let sync_root = directory.unwrap_or_else(|| env::current_dir().expect("Failed to get current directory"));
    let sync_root = sync_root.canonicalize().unwrap_or(sync_root);

    // Ensure .kitchensync directory exists
    let ks_dir = sync_root.join(".kitchensync");
    if !ks_dir.exists() {
        std::fs::create_dir_all(&ks_dir).expect("Failed to create .kitchensync directory");
    }

    // Check for peers.conf
    let peers_conf = ks_dir.join("peers.conf");
    if !peers_conf.exists() {
        println!("Error: No peers.conf found. Create .kitchensync/peers.conf with peer definitions.");
        std::process::exit(1);
    }

    // Parse configuration
    let global_config = match config::parse_peers_conf(&peers_conf) {
        Ok(cfg) => cfg,
        Err(e) => {
            println!("Configuration error: {}", e);
            std::process::exit(1);
        }
    };

    if global_config.peers.is_empty() {
        println!("Error: No peers configured in peers.conf");
        std::process::exit(1);
    }

    // Initialize local database
    let db_path = ks_dir.join("kitchensync.db");
    let local_db = match database::LocalDatabase::open(&db_path) {
        Ok(db) => Arc::new(std::sync::Mutex::new(db)),
        Err(e) => {
            println!("Database error: {}", e);
            std::process::exit(1);
        }
    };

    // Instance check and HTTP server
    let app_path = env::current_exe().expect("Failed to get executable path");
    let app_path = app_path.canonicalize().unwrap_or(app_path);

    {
        let db = local_db.lock().unwrap();
        if let Some(port) = db.get_config("serving-port") {
            if let Ok(port_num) = port.parse::<u16>() {
                // Check if another instance is running
                if http_server::check_instance(&app_path, port_num) {
                    // Another instance is running
                    return;
                }
            }
        }
    }

    // Bind to ephemeral port
    let (server, port) = http_server::start_server(&app_path);

    {
        let mut db = local_db.lock().unwrap();
        db.set_config("serving-port", &port.to_string());
        db.log("info", &format!("Startup, serving on port {}", port));
    }

    // Initialize peer databases
    let peer_dir = ks_dir.join("PEER");
    std::fs::create_dir_all(&peer_dir).ok();

    // Clean up peer databases for unlisted peers
    cleanup::cleanup_unlisted_peers(&peer_dir, &global_config.peers);

    // Create shutdown flag
    let shutdown_flag = Arc::new(AtomicBool::new(false));

    // Start HTTP server listener thread
    let shutdown_flag_http = shutdown_flag.clone();
    let http_handle = thread::spawn(move || {
        http_server::run_server(server, shutdown_flag_http);
    });

    // Create peer databases
    let mut peer_dbs = Vec::new();
    for peer in &global_config.peers {
        let peer_db_path = peer_dir.join(format!("{}.db", peer.name));
        match database::PeerDatabase::open(&peer_db_path) {
            Ok(db) => peer_dbs.push(Arc::new(std::sync::Mutex::new(db))),
            Err(e) => {
                println!("Failed to open peer database for {}: {}", peer.name, e);
                std::process::exit(1);
            }
        }
    }

    // Build ignore rules
    let ignore_matcher = ignore::build_ignore_matcher(&sync_root);

    // Start filesystem watcher (watch mode only)
    let watcher_handle = if !once_mode {
        let sync_root_clone = sync_root.clone();
        let local_db_clone = local_db.clone();
        let peer_dbs_clone: Vec<_> = peer_dbs.iter().map(Arc::clone).collect();
        let shutdown_clone = shutdown_flag.clone();
        let ignore_clone = ignore_matcher.clone();

        Some(thread::spawn(move || {
            watcher::run_watcher(
                sync_root_clone,
                local_db_clone,
                peer_dbs_clone,
                shutdown_clone,
                ignore_clone,
            );
        }))
    } else {
        None
    };

    // Run local walker
    {
        walker::run_local_walker(
            &sync_root,
            &local_db,
            &peer_dbs,
            &ignore_matcher,
        );
    }

    // Start connection managers
    let mut conn_handles = Vec::new();
    for (i, peer_config) in global_config.peers.iter().enumerate() {
        let sync_root_clone = sync_root.clone();
        let peer_config_clone = peer_config.clone();
        let global_config_clone = global_config.clone();
        let local_db_clone = local_db.clone();
        let peer_db = peer_dbs[i].clone();
        let all_peer_dbs: Vec<_> = peer_dbs.iter().map(Arc::clone).collect();
        let shutdown_clone = shutdown_flag.clone();
        let ignore_clone = ignore_matcher.clone();
        let once = once_mode;

        let handle = thread::spawn(move || {
            connection::run_connection_manager(
                sync_root_clone,
                peer_config_clone,
                global_config_clone,
                local_db_clone,
                peer_db,
                all_peer_dbs,
                shutdown_clone,
                ignore_clone,
                once,
            );
        });
        conn_handles.push(handle);
    }

    // Watch for peers.conf changes
    let peers_conf_clone = peers_conf.clone();
    let shutdown_conf = shutdown_flag.clone();
    let args_clone: Vec<String> = env::args().collect();
    let conf_watcher_handle = thread::spawn(move || {
        watcher::watch_config_file(peers_conf_clone, shutdown_conf, args_clone);
    });

    // Wait for all connection managers to complete
    for handle in conn_handles {
        handle.join().ok();
    }

    // In once mode, we're done after connection managers finish
    if once_mode {
        shutdown_flag.store(true, Ordering::SeqCst);
    }

    // Wait for watcher thread if in watch mode
    if let Some(handle) = watcher_handle {
        handle.join().ok();
    }

    conf_watcher_handle.join().ok();
    http_handle.join().ok();

    // Log shutdown
    {
        let mut db = local_db.lock().unwrap();
        db.log("info", "Shutdown complete");
    }
}
