mod backup;
mod cli;
mod decision;
mod entry;
mod local_transport;
mod peer;
mod sftp_transport;
mod snapshot;
mod staging;
mod sync_engine;
mod syncignore;
mod transport;

use cli::{parse_args, HELP_TEXT};
use log::LevelFilter;
use peer::{parse_peer, PeerRole};
use std::process;

fn validation_error(msg: &str) -> ! {
    println!("Error: {}", msg);
    println!();
    println!("{}", HELP_TEXT);
    process::exit(1);
}

fn main() {
    let args = parse_args();

    let level = match args.verbosity.as_str() {
        "error" => LevelFilter::Error,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Info,
    };
    // REQ_SYNCOP_023: All output goes to stdout
    env_logger::Builder::new()
        .filter_level(level)
        .target(env_logger::Target::Stdout)
        .init();

    let mut peers = Vec::new();
    for arg in &args.peers {
        match parse_peer(arg) {
            Ok(spec) => peers.push(spec),
            Err(e) => {
                validation_error(&format!("parsing peer '{}': {}", arg, e));
            }
        }
    }

    if peers.len() < 2 {
        validation_error("at least 2 peers are required");
    }

    // Check at most one canon peer
    let canon_count = peers.iter().filter(|p| p.role == PeerRole::Canon).count();
    if canon_count > 1 {
        validation_error("at most one canon (+) peer is allowed");
    }

    if let Err(e) = sync_engine::run_sync(
        &peers,
        args.max_connections,
        args.connect_timeout,
        args.staging_expiry_days,
        args.backup_expiry_days,
        args.tombstone_expiry_days,
    ) {
        println!("Sync failed: {}", e);
        process::exit(1);
    }
}
