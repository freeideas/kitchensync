#![allow(dead_code, unused_variables, unused_imports, unused_mut)]

mod config;
mod copy;
mod database;
mod hash;
mod lifecycle;
mod local_peer;
mod logging;
mod peer;
mod pool;
mod sftp_peer;
mod sync_engine;
mod timestamp;

use std::process;
use std::sync::Arc;
use std::time::Duration;

const HELP_TEXT: &str = r#"Usage: kitchensync <config> [OPTIONS]

Synchronize file trees across multiple peers.

Arguments:
  <config>  Path to config file, .kitchensync/ directory, or parent directory.

Options:
      --canon <peer>  Named peer is authoritative (its state always wins)
  -h, --help          Print this help

Config file resolution:
  1. Path to a .json file         -> use directly
  2. Path to a .kitchensync/ dir  -> append kitchensync-conf.json
  3. Path to any other dir        -> append .kitchensync/kitchensync-conf.json

Path resolution:
  All relative paths in the config resolve from the config file's directory.
  Peer URLs have one extra rule: .kitchensync/ can never be a sync target.
  If the config is inside a .kitchensync/ directory, peer URL paths back up
  to the parent of .kitchensync/ (so "." becomes ".."). Config at
  mydir/.kitchensync/kitchensync-conf.json with peer URL file://./
  refers to mydir/. This adjustment applies only to peer URLs, not to
  other settings like "database".

Setup:
  1. mkdir mydir/.kitchensync
  2. Create mydir/.kitchensync/kitchensync-conf.json:

     {
       peers: {
         nas:   { urls: ["sftp://user@host/path"] },
         local: { urls: ["file://./"] }
       }
     }

  3. Run: kitchensync mydir/

Full example config with all settings at their defaults (JSON5):

  {
    "database": "kitchensync.db",           // SQLite database path, relative to config dir
    "connection-timeout": 30,               // seconds for SSH connect to be aborted
    "max-connections": 10,                  // max concurrent connections per peer
    "xfer-cleanup-days": 2,                 // delete stale staging dirs after N days
    "back-retention-days": 90,              // delete displaced files after N days
    "tombstone-retention-days": 180,        // forget deletion records after N days
    "log-retention-days": 32,               // purge log entries after N days

    // Peers: at least two required. URLs tried top-to-bottom; first success wins.
    peers: {
      nas: {
        urls: [
          "sftp://bilbo@192.168.1.50/volume1/docs",
          "sftp://bilbo@nas.tail12345.ts.net/volume1/docs"
        ]
      },
      laptop: {
        urls: [
          "sftp://bilbo@laptop.local/home/bilbo/docs",
          "sftp://bilbo@laptop.tail12345.ts.net/home/bilbo/docs"
        ]
      },
      usb: {
        urls: ["file:///media/bilbo/usb-backup/docs"]
      }
    }
  }

URL schemes:
  sftp://user@host/path              Remote over SSH (port 22)
  sftp://user@host:port/path         Non-standard SSH port
  sftp://user:password@host/path     Inline password (prefer SSH keys)
  file:///absolute/path              Local, absolute
  file://./relative/path             Local, relative to config dir

  Percent-encode special characters in passwords (@ -> %40, : -> %3A).
  SFTP paths are absolute from filesystem root.

Authentication (fallback chain, stops at first success):
  1. Inline password from URL
  2. SSH agent (SSH_AUTH_SOCK)
  3. ~/.ssh/id_ed25519
  4. ~/.ssh/id_ecdsa
  5. ~/.ssh/id_rsa

  Host keys verified via ~/.ssh/known_hosts. Unknown hosts rejected.

Peer names:
  Must match [a-zA-Z0-9][a-zA-Z0-9_-]*, max 64 characters."#;

struct Args {
    config_path: String,
    canon: Option<String>,
    help: bool,
}

fn parse_args() -> Result<Args, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        return Err("Missing <config> argument. Use -h for help.".to_string());
    }

    let mut config_path = None;
    let mut canon = None;
    let mut help = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                help = true;
            }
            "--canon" => {
                i += 1;
                if i >= args.len() {
                    return Err("--canon requires a peer name".to_string());
                }
                canon = Some(args[i].clone());
            }
            _ => {
                if config_path.is_none() {
                    config_path = Some(args[i].clone());
                } else {
                    return Err(format!("Unexpected argument: {}", args[i]));
                }
            }
        }
        i += 1;
    }

    if help {
        return Ok(Args {
            config_path: String::new(),
            canon: None,
            help: true,
        });
    }

    match config_path {
        Some(p) => Ok(Args {
            config_path: p,
            canon,
            help: false,
        }),
        None => Err("Missing <config> argument. Use -h for help.".to_string()),
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            println!("{}", e);
            process::exit(1);
        }
    };

    if args.help {
        println!("{}", HELP_TEXT);
        process::exit(0);
    }

    // Resolve config file
    let config_file = match config::resolve_config_path(&args.config_path) {
        Ok(p) => p,
        Err(e) => {
            println!("{}", e);
            process::exit(1);
        }
    };

    // Load config
    let cfg = match config::load_config(&config_file) {
        Ok(c) => c,
        Err(e) => {
            println!("{}", e);
            process::exit(1);
        }
    };

    // Validate --canon peer exists in config
    if let Some(ref canon_name) = args.canon {
        if !cfg.peers.contains_key(canon_name) {
            println!("Unknown peer in --canon: {}", canon_name);
            process::exit(1);
        }
    }

    // Open database
    let db_path = config::resolve_database_path(&cfg);
    let db = match database::Database::open(&db_path) {
        Ok(d) => Arc::new(d),
        Err(e) => {
            println!("{}", e);
            process::exit(1);
        }
    };

    let config_canonical = cfg
        .config_file_path
        .to_string_lossy()
        .to_string();

    // Instance check
    if let Some(port_str) = db.get_config("serving-port") {
        if let Ok(port) = port_str.parse::<u16>() {
            if lifecycle::check_existing_instance(port, &config_canonical) {
                println!("Already running against {}", config_canonical);
                process::exit(0);
            }
        }
    }

    // Start lifecycle HTTP server
    let server = match lifecycle::LifecycleServer::start(&db, &config_canonical) {
        Ok(s) => s,
        Err(e) => {
            println!("Lifecycle server error: {}", e);
            process::exit(1);
        }
    };

    let logger = Arc::new(logging::Logger::new(db.clone(), cfg.log_retention_days));
    logger.info("kitchensync starting");

    // Connect to all peers in parallel
    let peers: Vec<Arc<pool::ConnectedPeer>> = {
        let results: Vec<(String, Result<pool::ConnectedPeer, peer::PeerError>)> =
            std::thread::scope(|s| {
                let handles: Vec<_> = cfg
                    .peers
                    .values()
                    .map(|pc| {
                        let name = pc.name.clone();
                        let pc = pc.clone();
                        let max_conn = cfg.max_connections;
                        let timeout = cfg.connection_timeout;
                        let logger = logger.clone();
                        s.spawn(move || {
                            let result = pool::ConnectedPeer::connect(&pc, max_conn, timeout, logger);
                            (name, result)
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });

        let mut connected = Vec::new();
        for (name, result) in results {
            match result {
                Ok(p) => connected.push(Arc::new(p)),
                Err(e) => {
                    logger.error(&format!("Peer {} unreachable: {}", name, e));
                }
            }
        }
        connected
    };

    // Check canon peer is reachable
    if let Some(ref canon_name) = args.canon {
        if !peers.iter().any(|p| p.name == *canon_name) {
            println!("Canon peer '{}' is unreachable", canon_name);
            process::exit(1);
        }
    }

    // Check minimum reachable peers
    if args.canon.is_none() && peers.len() < 2 {
        println!(
            "Need at least 2 reachable peers, only {} connected",
            peers.len()
        );
        process::exit(1);
    }
    if args.canon.is_some() && peers.is_empty() {
        println!("No reachable peers");
        process::exit(1);
    }

    // Purge old data
    db.purge_tombstones(cfg.tombstone_retention_days);
    db.purge_logs(cfg.log_retention_days);

    // Run sync
    let sync_stamp = timestamp::now();
    sync_engine::run(
        &peers,
        &db,
        &logger,
        args.canon.as_deref(),
        &sync_stamp,
        &cfg,
    );

    logger.info("kitchensync complete");

    // Disconnect peers
    drop(peers);

    // Post-completion linger (5 seconds)
    server.linger(Duration::from_secs(5));
}
