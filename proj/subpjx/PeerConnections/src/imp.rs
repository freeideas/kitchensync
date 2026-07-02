use crate::api::*;
use std::sync::Arc;

struct PeerConnectionsImpl {
    fileurlconnection: std::sync::Arc<dyn peerconnections_fileurlconnection::FileUrlConnection>,
    sftpurlconnection: std::sync::Arc<dyn peerconnections_sftpurlconnection::SftpUrlConnection>,
    startupcoordinator: std::sync::Arc<dyn peerconnections_startupcoordinator::StartupCoordinator>,
}

impl PeerConnections for PeerConnectionsImpl {
    fn establish_peer_connections(
        &self,
        request: PeerConnectionStartupRequest,
    ) -> PeerConnectionStartupResult {
        let result = self.startupcoordinator.coordinate_startup(
            peerconnections_startupcoordinator::StartupCoordinatorRequest {
                peers: request.peers.into_iter().map(startup_peer).collect(),
                global_connection: peerconnections_startupcoordinator::StartupCoordinatorGlobalSettings {
                    timeout_conn_seconds: request.global_connection.timeout_conn_seconds,
                    timeout_idle_seconds: request.global_connection.timeout_idle_seconds,
                },
                run_mode: startup_run_mode(request.run_mode),
                local_environment: peerconnections_startupcoordinator::StartupCoordinatorLocalEnvironment {
                    home_directory: request.local_environment.home_directory,
                    known_hosts_path: request.local_environment.known_hosts_path,
                    ssh_agent_socket: request.local_environment.ssh_agent_socket,
                },
            },
        );

        PeerConnectionStartupResult {
            reachable_peers: result
                .reachable_peers
                .into_iter()
                .map(reachable_peer)
                .collect(),
            unreachable_peers: result
                .unreachable_peers
                .into_iter()
                .map(unreachable_peer)
                .collect(),
            status: startup_status(result.status),
        }
    }
}

pub fn new(
    fileurlconnection: std::sync::Arc<dyn peerconnections_fileurlconnection::FileUrlConnection>,
    sftpurlconnection: std::sync::Arc<dyn peerconnections_sftpurlconnection::SftpUrlConnection>,
    startupcoordinator: std::sync::Arc<dyn peerconnections_startupcoordinator::StartupCoordinator>,
) -> std::sync::Arc<dyn PeerConnections> {
    Arc::new(PeerConnectionsImpl {
        fileurlconnection,
        sftpurlconnection,
        startupcoordinator,
    })
}

fn startup_peer(
    peer: PeerConnectionPeer,
) -> peerconnections_startupcoordinator::StartupCoordinatorPeer {
    let mut urls = peer.urls.into_iter();
    let primary_url = urls
        .next()
        .map(startup_url)
        .expect("validated peer must include a primary URL");

    peerconnections_startupcoordinator::StartupCoordinatorPeer {
        identity: peer.identity,
        role: startup_peer_role(peer.role),
        primary_url,
        fallback_urls: urls.map(startup_url).collect(),
    }
}

fn startup_url(
    url: PeerConnectionUrl,
) -> peerconnections_startupcoordinator::StartupCoordinatorUrl {
    peerconnections_startupcoordinator::StartupCoordinatorUrl {
        normalized_identity: url.normalized_identity,
        location: startup_location(url.location),
        connection: peerconnections_startupcoordinator::StartupCoordinatorUrlSettings {
            timeout_conn_seconds: url.connection.timeout_conn_seconds,
            timeout_idle_seconds: url.connection.timeout_idle_seconds,
        },
    }
}

fn startup_location(
    location: PeerConnectionLocation,
) -> peerconnections_startupcoordinator::StartupCoordinatorUrlLocation {
    match location {
        PeerConnectionLocation::Local(local) => {
            peerconnections_startupcoordinator::StartupCoordinatorUrlLocation::File(
                peerconnections_startupcoordinator::StartupCoordinatorFileUrl {
                    local_peer_root_path: local.path_or_url.into(),
                },
            )
        }
        PeerConnectionLocation::Sftp(sftp) => {
            peerconnections_startupcoordinator::StartupCoordinatorUrlLocation::Sftp(
                peerconnections_startupcoordinator::StartupCoordinatorSftpUrl {
                    host: sftp.host,
                    username: sftp.username,
                    password: sftp.password,
                    port: sftp.port,
                    absolute_path: sftp.absolute_path,
                },
            )
        }
    }
}

fn startup_peer_role(
    role: PeerConnectionPeerRole,
) -> peerconnections_startupcoordinator::StartupCoordinatorPeerRole {
    match role {
        PeerConnectionPeerRole::Canon => {
            peerconnections_startupcoordinator::StartupCoordinatorPeerRole::Canon
        }
        PeerConnectionPeerRole::Subordinate => {
            peerconnections_startupcoordinator::StartupCoordinatorPeerRole::Subordinate
        }
        PeerConnectionPeerRole::Normal => {
            peerconnections_startupcoordinator::StartupCoordinatorPeerRole::Normal
        }
    }
}

fn startup_run_mode(
    run_mode: PeerConnectionRunMode,
) -> peerconnections_startupcoordinator::StartupCoordinatorRunMode {
    match run_mode {
        PeerConnectionRunMode::Normal => {
            peerconnections_startupcoordinator::StartupCoordinatorRunMode::Normal
        }
        PeerConnectionRunMode::DryRun => {
            peerconnections_startupcoordinator::StartupCoordinatorRunMode::DryRun
        }
    }
}

fn reachable_peer(
    peer: peerconnections_startupcoordinator::StartupCoordinatorReachablePeer,
) -> ReachablePeerConnection {
    ReachablePeerConnection {
        peer_identity: peer.peer_identity,
        role: peer_role(peer.role),
        winning_url: peer_connection_url(peer.winning_url),
        effective_sftp_connection: effective_sftp_connection(peer.connection),
    }
}

fn unreachable_peer(
    peer: peerconnections_startupcoordinator::StartupCoordinatorUnreachablePeer,
) -> UnreachablePeerConnection {
    UnreachablePeerConnection {
        peer_identity: peer.peer_identity,
        role: peer_role(peer.role),
        diagnostic: PeerConnectionDiagnostic {
            kind: match peer.diagnostic.kind {
                peerconnections_startupcoordinator::StartupCoordinatorErrorDiagnosticKind::UnreachablePeer => PeerConnectionDiagnosticKind::UnreachablePeer,
            },
            details: peer.diagnostic.details,
        },
    }
}

fn peer_connection_url(
    url: peerconnections_startupcoordinator::StartupCoordinatorUrl,
) -> PeerConnectionUrl {
    PeerConnectionUrl {
        normalized_identity: url.normalized_identity,
        location: peer_connection_location(url.location),
        connection: PeerConnectionUrlSettings {
            timeout_conn_seconds: url.connection.timeout_conn_seconds,
            timeout_idle_seconds: url.connection.timeout_idle_seconds,
        },
    }
}

fn peer_connection_location(
    location: peerconnections_startupcoordinator::StartupCoordinatorUrlLocation,
) -> PeerConnectionLocation {
    match location {
        peerconnections_startupcoordinator::StartupCoordinatorUrlLocation::File(file) => {
            PeerConnectionLocation::Local(PeerConnectionLocalUrl {
                path_or_url: file.local_peer_root_path.to_string_lossy().into_owned(),
            })
        }
        peerconnections_startupcoordinator::StartupCoordinatorUrlLocation::Sftp(sftp) => {
            PeerConnectionLocation::Sftp(PeerConnectionSftpUrl {
                host: sftp.host,
                username: sftp.username,
                password: sftp.password,
                port: sftp.port,
                absolute_path: sftp.absolute_path,
            })
        }
    }
}

fn peer_role(
    role: peerconnections_startupcoordinator::StartupCoordinatorPeerRole,
) -> PeerConnectionPeerRole {
    match role {
        peerconnections_startupcoordinator::StartupCoordinatorPeerRole::Canon => {
            PeerConnectionPeerRole::Canon
        }
        peerconnections_startupcoordinator::StartupCoordinatorPeerRole::Subordinate => {
            PeerConnectionPeerRole::Subordinate
        }
        peerconnections_startupcoordinator::StartupCoordinatorPeerRole::Normal => {
            PeerConnectionPeerRole::Normal
        }
    }
}

fn effective_sftp_connection(
    connection: peerconnections_startupcoordinator::StartupCoordinatorConnection,
) -> Option<PeerConnectionEffectiveSftpSettings> {
    match connection {
        peerconnections_startupcoordinator::StartupCoordinatorConnection::File(_) => None,
        peerconnections_startupcoordinator::StartupCoordinatorConnection::Sftp(sftp) => {
            Some(PeerConnectionEffectiveSftpSettings {
                timeout_conn_seconds: sftp.timeout_conn_seconds,
                timeout_idle_seconds: sftp.timeout_idle_seconds,
            })
        }
    }
}

fn startup_status(
    status: peerconnections_startupcoordinator::StartupCoordinatorStatus,
) -> PeerConnectionStartupStatus {
    match status {
        peerconnections_startupcoordinator::StartupCoordinatorStatus::Ready => {
            PeerConnectionStartupStatus::Ready
        }
        peerconnections_startupcoordinator::StartupCoordinatorStatus::Fatal(reasons) => {
            PeerConnectionStartupStatus::Fatal(
                reasons.into_iter().map(fatal_reason).collect(),
            )
        }
    }
}

fn fatal_reason(
    reason: peerconnections_startupcoordinator::StartupCoordinatorFatalReason,
) -> PeerConnectionFatalStartupReason {
    match reason {
        peerconnections_startupcoordinator::StartupCoordinatorFatalReason::FewerThanTwoReachablePeers => {
            PeerConnectionFatalStartupReason::FewerThanTwoReachablePeers
        }
        peerconnections_startupcoordinator::StartupCoordinatorFatalReason::CanonPeerUnreachable => {
            PeerConnectionFatalStartupReason::CanonPeerUnreachable
        }
    }
}
