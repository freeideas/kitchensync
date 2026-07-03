use commandline::{CommandLinePeerRole, CommandLineProcessOutput};
use peerconnections::{PeerConnectionsPeerRole, PeerConnectionsStartupResult};
use snapshotdatabase::{SnapshotDatabaseUploadRequest, SnapshotDatabaseUploadResult};
use synctraversal::{SyncTraversalPeer, SyncTraversalPeerRole, SyncTraversalRequest};

fn main() {
    let cli = commandline::new();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output = match cli.parse(args) {
        commandline::CommandLineParseResult::Help => cli.help_output(),
        commandline::CommandLineParseResult::ValidationError(error) => {
            cli.validation_error_output(&error)
        }
        commandline::CommandLineParseResult::Run(request) => run_sync(cli.as_ref(), request),
    };
    print!("{}", output.stdout);
    std::process::exit(output.exit_code);
}

fn run_sync(
    cli: &dyn commandline::CommandLine,
    request: commandline::CommandLineRunRequest,
) -> CommandLineProcessOutput {
    let formatrules = formatrules::new();
    let peertransportsurface = peertransportsurface::new();
    let snapshotdatabase = snapshotdatabase::new(
        formatrules.clone(),
        peertransportsurface.clone(),
    );
    let peerconnections = peerconnections::new(
        formatrules.clone(),
        peertransportsurface.clone(),
        snapshotdatabase.clone(),
    );
    let copystaging = copystaging::new(
        formatrules.clone(),
        peertransportsurface.clone(),
    );
    let synctraversal = synctraversal::new(
        formatrules,
        peertransportsurface,
        snapshotdatabase.clone(),
        copystaging,
    );

    let startup = match peerconnections.start(peerconnections::PeerConnectionsStartupRequest {
        dry_run: request.settings.dry_run,
        timeout_conn_seconds: request.settings.timeout_conn_seconds,
        timeout_idle_seconds: request.settings.timeout_idle_seconds,
        peer_arguments: peer_arguments(&request.peers),
    }) {
        PeerConnectionsStartupResult::Ready(startup) => startup,
        PeerConnectionsStartupResult::Failed(failure) => {
            return startup_failure_output(failure.reason);
        }
    };

    let traversal_peers = startup
        .peers
        .iter()
        .map(|peer| SyncTraversalPeer {
            peer_index: peer.peer_index,
            peer_url: peer.winning_url.clone(),
            role: traversal_role(peer.role),
            had_snapshot_history: peer.had_snapshot_history,
            root: peer.root.clone(),
            snapshot_database: snapshotdatabase::SnapshotDatabasePeerDatabase {
                peer_index: peer.peer_index,
                local_snapshot_path: peer.snapshot_database.path.clone(),
            },
        })
        .collect::<Vec<_>>();

    let traversal = synctraversal.traverse(SyncTraversalRequest {
        peers: traversal_peers,
        retries_list: request.settings.retries_list,
        excludes: request.settings.excludes,
    });
    if !traversal.diagnostics.is_empty() {
        return CommandLineProcessOutput {
            stdout: String::new(),
            exit_code: 1,
        };
    }

    if !request.settings.dry_run {
        for peer in startup.peers {
            let upload = snapshotdatabase.upload_snapshot(SnapshotDatabaseUploadRequest {
                peer_index: peer.peer_index,
                peer: peer.root,
                local_snapshot_path: peer.snapshot_database.path,
            });
            if !matches!(upload, SnapshotDatabaseUploadResult::Uploaded) {
                return CommandLineProcessOutput {
                    stdout: String::new(),
                    exit_code: 1,
                };
            }
        }
    }

    cli.sync_complete_output()
}

fn peer_arguments(peers: &[commandline::CommandLinePeer]) -> Vec<String> {
    peers.iter().map(peer_argument).collect()
}

fn peer_argument(peer: &commandline::CommandLinePeer) -> String {
    let prefix = match peer.role {
        CommandLinePeerRole::Normal => "",
        CommandLinePeerRole::Canon => "+",
        CommandLinePeerRole::Subordinate => "-",
    };
    let urls = peer
        .urls
        .iter()
        .map(|url| url.url.as_str())
        .collect::<Vec<_>>();
    if urls.len() == 1 {
        format!("{}{}", prefix, urls[0])
    } else {
        format!("{}[{}]", prefix, urls.join(","))
    }
}

fn traversal_role(role: PeerConnectionsPeerRole) -> SyncTraversalPeerRole {
    match role {
        PeerConnectionsPeerRole::Normal => SyncTraversalPeerRole::Normal,
        PeerConnectionsPeerRole::Canon => SyncTraversalPeerRole::Canon,
        PeerConnectionsPeerRole::Subordinate => SyncTraversalPeerRole::Subordinate,
    }
}

fn startup_failure_output(
    reason: peerconnections::PeerConnectionsStartupFailureReason,
) -> CommandLineProcessOutput {
    let stdout = match reason {
        peerconnections::PeerConnectionsStartupFailureReason::FirstSyncNeedsCanon => {
            "First sync? Mark the authoritative peer with a leading +\n"
        }
        peerconnections::PeerConnectionsStartupFailureReason::NoContributingPeerReachable => {
            "No contributing peer reachable - cannot make sync decisions\n"
        }
        _ => "sync failed\n",
    };
    CommandLineProcessOutput {
        stdout: stdout.to_string(),
        exit_code: 1,
    }
}
