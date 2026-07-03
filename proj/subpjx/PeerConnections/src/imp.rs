use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use crate::api::*;

static NEXT_LOCAL_SNAPSHOT: AtomicU64 = AtomicU64::new(0);

struct PeerConnectionsImpl {
    formatrules: std::sync::Arc<dyn formatrules::FormatRules>,
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
    snapshotdatabase: std::sync::Arc<dyn snapshotdatabase::SnapshotDatabase>,
}

#[derive(Clone, Copy)]
enum ParsedRole {
    Normal,
    Canon,
    Subordinate,
}

struct ParsedPeer {
    peer_index: usize,
    role: ParsedRole,
    candidates: Vec<String>,
}

struct ConnectedPeer {
    peer_index: usize,
    role: ParsedRole,
    winning_url: String,
    peer: peertransportsurface::ConnectedPeerRoot,
}

struct PreparedPeer {
    peer_index: usize,
    role: ParsedRole,
    had_snapshot_history: bool,
    winning_url: String,
    peer: peertransportsurface::ConnectedPeerRoot,
    snapshot_path: PathBuf,
}

impl PeerConnections for PeerConnectionsImpl {
    fn start(
        &self,
        request: PeerConnectionsStartupRequest,
    ) -> PeerConnectionsStartupResult {
        let _ = &self.peertransportsurface;
        let parsed_peers = parse_peer_arguments(&request.peer_arguments);
        let canon_peer_index = parsed_peers
            .iter()
            .find(|peer| matches!(peer.role, ParsedRole::Canon))
            .map(|peer| peer.peer_index);
        let connected_peers = connect_peers(
            parsed_peers,
            self.formatrules.clone(),
            request.dry_run,
        );

        let mut diagnostics = Vec::new();
        let mut reachable = Vec::new();
        for (peer_index, connected) in connected_peers {
            match connected {
                Some(peer) => reachable.push(peer),
                None => diagnostics.push(unreachable_diagnostic(peer_index)),
            }
        }

        if let Some(failure) = reachable_set_failure(&reachable, canon_peer_index, &diagnostics) {
            return PeerConnectionsStartupResult::Failed(failure);
        }

        let prepared = prepare_snapshots(
            reachable,
            request.dry_run,
            self.snapshotdatabase.as_ref(),
            &mut diagnostics,
        );

        if let Some(failure) = reachable_set_failure(&prepared, canon_peer_index, &diagnostics) {
            return PeerConnectionsStartupResult::Failed(failure);
        }

        if !prepared.iter().any(|peer| peer.had_snapshot_history) && canon_peer_index.is_none() {
            return PeerConnectionsStartupResult::Failed(PeerConnectionsStartupFailure {
                reason: PeerConnectionsStartupFailureReason::FirstSyncNeedsCanon,
                diagnostics,
            });
        }

        let peers = prepared
            .into_iter()
            .map(|peer| peer.into_startup_peer())
            .collect::<Vec<_>>();

        if peers
            .iter()
            .all(|peer| peer.role == PeerConnectionsPeerRole::Subordinate)
        {
            return PeerConnectionsStartupResult::Failed(PeerConnectionsStartupFailure {
                reason: PeerConnectionsStartupFailureReason::NoContributingPeerReachable,
                diagnostics,
            });
        }

        PeerConnectionsStartupResult::Ready(PeerConnectionsStartup { peers, diagnostics })
    }
}

impl PreparedPeer {
    fn into_startup_peer(self) -> PeerConnectionsPeer {
        PeerConnectionsPeer {
            peer_index: self.peer_index,
            role: final_role(self.role, self.had_snapshot_history),
            had_snapshot_history: self.had_snapshot_history,
            winning_url: self.winning_url,
            transport_handle: PeerConnectionsTransportHandle {
                handle: Arc::new(self.peer.clone()),
            },
            snapshot_database: PeerConnectionsSnapshotDatabase {
                path: self.snapshot_path,
            },
        }
    }
}

fn parse_peer_arguments(peer_arguments: &[String]) -> Vec<ParsedPeer> {
    peer_arguments
        .iter()
        .enumerate()
        .map(|(peer_index, argument)| {
            let (role, value) = match argument.as_bytes().first() {
                Some(b'+') => (ParsedRole::Canon, &argument[1..]),
                Some(b'-') => (ParsedRole::Subordinate, &argument[1..]),
                _ => (ParsedRole::Normal, argument.as_str()),
            };
            let candidates = if value.starts_with('[') && value.ends_with(']') {
                value[1..value.len() - 1]
                    .split(',')
                    .map(str::to_string)
                    .collect()
            } else {
                vec![value.to_string()]
            };
            ParsedPeer {
                peer_index,
                role,
                candidates,
            }
        })
        .collect()
}

fn connect_peers(
    peers: Vec<ParsedPeer>,
    formatrules: Arc<dyn formatrules::FormatRules>,
    dry_run: bool,
) -> Vec<(usize, Option<ConnectedPeer>)> {
    let current_working_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let os_username = std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok());

    thread::scope(|scope| {
        let handles = peers
            .into_iter()
            .map(|peer| {
                let formatrules = formatrules.clone();
                let current_working_directory = current_working_directory.clone();
                let os_username = os_username.clone();
                scope.spawn(move || {
                    let peer_index = peer.peer_index;
                    (
                        peer_index,
                        connect_peer(
                            peer,
                            formatrules.as_ref(),
                            current_working_directory,
                            os_username,
                            dry_run,
                        ),
                    )
                })
            })
            .collect::<Vec<_>>();

        handles
            .into_iter()
            .map(|handle| match handle.join() {
                Ok(result) => result,
                Err(_) => panic!("peer connection thread panicked"),
            })
            .collect()
    })
}

fn connect_peer(
    peer: ParsedPeer,
    formatrules: &dyn formatrules::FormatRules,
    current_working_directory: PathBuf,
    os_username: Option<String>,
    dry_run: bool,
) -> Option<ConnectedPeer> {
    for candidate in peer.candidates {
        let winning_url = match formatrules.normalize_peer_identity(
            formatrules::FormatRulesPeerIdentityRequest {
                peer_url: candidate,
                current_working_directory: current_working_directory.clone(),
                os_username: os_username.clone(),
            },
        ) {
            Ok(url) => url,
            Err(_) => continue,
        };

        let root_path = match file_url_path(&winning_url) {
            Some(path) => path,
            None => continue,
        };
        if ensure_local_root(&root_path, !dry_run).is_err() {
            continue;
        }

        return Some(ConnectedPeer {
            peer_index: peer.peer_index,
            role: peer.role,
            winning_url,
            peer: peertransportsurface::ConnectedPeerRoot {
                handle: Arc::new(root_path),
            },
        });
    }
    None
}

fn file_url_path(url: &str) -> Option<PathBuf> {
    let path = url.strip_prefix("file://")?;
    #[cfg(windows)]
    {
        let path = path
            .strip_prefix('/')
            .filter(|value| looks_like_windows_drive_path(value))
            .unwrap_or(path);
        Some(PathBuf::from(path.replace('/', "\\")))
    }
    #[cfg(not(windows))]
    {
        Some(PathBuf::from(path))
    }
}

#[cfg(windows)]
fn looks_like_windows_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'/'
}

fn ensure_local_root(path: &Path, create_missing_root: bool) -> Result<(), ()> {
    if create_missing_root {
        fs::create_dir_all(path).map_err(|_| ())?;
    }
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Err(()),
        Err(_) => Err(()),
    }
}

fn prepare_snapshots(
    reachable: Vec<ConnectedPeer>,
    dry_run: bool,
    snapshotdatabase: &dyn snapshotdatabase::SnapshotDatabase,
    diagnostics: &mut Vec<PeerConnectionsDiagnostic>,
) -> Vec<PreparedPeer> {
    let mode = if dry_run {
        snapshotdatabase::SnapshotDatabaseRunMode::DryRun
    } else {
        snapshotdatabase::SnapshotDatabaseRunMode::Normal
    };
    let mut prepared = Vec::new();

    for peer in reachable {
        let local_snapshot_path = local_snapshot_path(peer.peer_index);
        match snapshotdatabase.prepare_peer_snapshot(
            snapshotdatabase::SnapshotDatabasePrepareRequest {
                peer_index: peer.peer_index,
                peer: peer.peer.clone(),
                local_snapshot_path,
                mode,
            },
        ) {
            snapshotdatabase::SnapshotDatabasePrepareResult::Prepared(result) => {
                prepared.push(PreparedPeer {
                    peer_index: peer.peer_index,
                    role: peer.role,
                    had_snapshot_history: result.had_snapshot_history,
                    winning_url: peer.winning_url,
                    peer: peer.peer,
                    snapshot_path: result.local_snapshot_path,
                });
            }
            snapshotdatabase::SnapshotDatabasePrepareResult::Excluded(_) => {
                diagnostics.push(PeerConnectionsDiagnostic {
                    level: PeerConnectionsDiagnosticLevel::Error,
                    peer_index: peer.peer_index,
                    kind: PeerConnectionsDiagnosticKind::SnapshotStartupFailed,
                });
            }
        }
    }

    prepared
}

fn local_snapshot_path(peer_index: usize) -> PathBuf {
    let unique_id = NEXT_LOCAL_SNAPSHOT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "kitchensync-{}-{}-peer-{}-snapshot.db",
        std::process::id(),
        unique_id,
        peer_index
    ))
}

fn final_role(role: ParsedRole, had_snapshot_history: bool) -> PeerConnectionsPeerRole {
    match role {
        ParsedRole::Canon => PeerConnectionsPeerRole::Canon,
        ParsedRole::Subordinate => PeerConnectionsPeerRole::Subordinate,
        ParsedRole::Normal if had_snapshot_history => PeerConnectionsPeerRole::Normal,
        ParsedRole::Normal => PeerConnectionsPeerRole::Subordinate,
    }
}

fn unreachable_diagnostic(peer_index: usize) -> PeerConnectionsDiagnostic {
    PeerConnectionsDiagnostic {
        level: PeerConnectionsDiagnosticLevel::Error,
        peer_index,
        kind: PeerConnectionsDiagnosticKind::PeerUnreachable,
    }
}

fn reachable_set_failure<T: StartupReachablePeer>(
    peers: &[T],
    canon_peer_index: Option<usize>,
    diagnostics: &[PeerConnectionsDiagnostic],
) -> Option<PeerConnectionsStartupFailure> {
    if let Some(canon_peer_index) = canon_peer_index {
        if !peers.iter().any(|peer| peer.peer_index() == canon_peer_index) {
            return Some(PeerConnectionsStartupFailure {
                reason: PeerConnectionsStartupFailureReason::CanonPeerUnreachable,
                diagnostics: diagnostics.to_vec(),
            });
        }
    }
    if peers.len() < 2 {
        return Some(PeerConnectionsStartupFailure {
            reason: PeerConnectionsStartupFailureReason::FewerThanTwoReachablePeers,
            diagnostics: diagnostics.to_vec(),
        });
    }
    None
}

trait StartupReachablePeer {
    fn peer_index(&self) -> usize;
}

impl StartupReachablePeer for ConnectedPeer {
    fn peer_index(&self) -> usize {
        self.peer_index
    }
}

impl StartupReachablePeer for PreparedPeer {
    fn peer_index(&self) -> usize {
        self.peer_index
    }
}

pub fn new(
    formatrules: std::sync::Arc<dyn formatrules::FormatRules>,
    peertransportsurface: std::sync::Arc<dyn peertransportsurface::PeerTransportSurface>,
    snapshotdatabase: std::sync::Arc<dyn snapshotdatabase::SnapshotDatabase>,
) -> std::sync::Arc<dyn PeerConnections> {
    Arc::new(PeerConnectionsImpl {
        formatrules,
        peertransportsurface,
        snapshotdatabase,
    })
}
