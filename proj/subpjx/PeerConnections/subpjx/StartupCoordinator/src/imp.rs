use std::sync::Arc;
use crate::api::*;

struct StartupCoordinatorImpl {
    fileurlconnection: std::sync::Arc<dyn peerconnections_fileurlconnection::FileUrlConnection>,
    sftpurlconnection: std::sync::Arc<dyn peerconnections_sftpurlconnection::SftpUrlConnection>,
}

impl StartupCoordinator for StartupCoordinatorImpl {
    fn coordinate_startup( &self, request: StartupCoordinatorRequest, ) -> StartupCoordinatorResult {
        let mut peer_results = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for (index, peer) in request.peers.iter().cloned().enumerate() {
                let fileurlconnection = Arc::clone(&self.fileurlconnection);
                let sftpurlconnection = Arc::clone(&self.sftpurlconnection);
                let global_connection = request.global_connection;
                let run_mode = request.run_mode;
                let local_environment = request.local_environment.clone();

                handles.push(scope.spawn(move || {
                    (
                        index,
                        coordinate_peer(
                            peer,
                            fileurlconnection,
                            sftpurlconnection,
                            global_connection,
                            run_mode,
                            local_environment,
                        ),
                    )
                }));
            }

            handles
                .into_iter()
                .map(|handle| handle.join().expect("startup worker should not panic"))
                .collect::<Vec<_>>()
        });

        peer_results.sort_by_key(|(index, _)| *index);

        let mut reachable_peers = Vec::new();
        let mut unreachable_peers = Vec::new();
        for (_, peer_result) in peer_results {
            match peer_result {
                PeerStartupResult::Reachable(peer) => reachable_peers.push(peer),
                PeerStartupResult::Unreachable(peer) => unreachable_peers.push(peer),
            }
        }

        let mut fatal_reasons = Vec::new();
        if reachable_peers.len() < 2 {
            fatal_reasons.push(StartupCoordinatorFatalReason::FewerThanTwoReachablePeers);
        }
        if unreachable_peers
            .iter()
            .any(|peer| peer.role == StartupCoordinatorPeerRole::Canon)
        {
            fatal_reasons.push(StartupCoordinatorFatalReason::CanonPeerUnreachable);
        }

        StartupCoordinatorResult {
            reachable_peers,
            unreachable_peers,
            status: if fatal_reasons.is_empty() {
                StartupCoordinatorStatus::Ready
            } else {
                StartupCoordinatorStatus::Fatal(fatal_reasons)
            },
        }
    }
}

pub fn new(fileurlconnection: std::sync::Arc<dyn peerconnections_fileurlconnection::FileUrlConnection>, sftpurlconnection: std::sync::Arc<dyn peerconnections_sftpurlconnection::SftpUrlConnection>) -> std::sync::Arc<dyn StartupCoordinator> {
    Arc::new(StartupCoordinatorImpl { fileurlconnection, sftpurlconnection })
}

enum PeerStartupResult {
    Reachable(StartupCoordinatorReachablePeer),
    Unreachable(StartupCoordinatorUnreachablePeer),
}

fn coordinate_peer(
    peer: StartupCoordinatorPeer,
    fileurlconnection: Arc<dyn peerconnections_fileurlconnection::FileUrlConnection>,
    sftpurlconnection: Arc<dyn peerconnections_sftpurlconnection::SftpUrlConnection>,
    global_connection: StartupCoordinatorGlobalSettings,
    run_mode: StartupCoordinatorRunMode,
    local_environment: StartupCoordinatorLocalEnvironment,
) -> PeerStartupResult {
    let mut last_failure = String::new();
    let mut urls = std::iter::once(peer.primary_url).chain(peer.fallback_urls);

    for url in urls.by_ref() {
        match establish_url(
            &url,
            &fileurlconnection,
            &sftpurlconnection,
            global_connection,
            run_mode,
            &local_environment,
        ) {
            Ok(connection) => {
                return PeerStartupResult::Reachable(StartupCoordinatorReachablePeer {
                    peer_identity: peer.identity,
                    role: peer.role,
                    winning_url: url,
                    connection,
                });
            }
            Err(failure) => last_failure = failure,
        }
    }

    PeerStartupResult::Unreachable(StartupCoordinatorUnreachablePeer {
        peer_identity: peer.identity.clone(),
        role: peer.role,
        diagnostic: StartupCoordinatorErrorDiagnostic {
            kind: StartupCoordinatorErrorDiagnosticKind::UnreachablePeer,
            peer_identity: peer.identity,
            details: if last_failure.is_empty() {
                "peer has no startup URLs".to_string()
            } else {
                last_failure
            },
        },
    })
}

fn establish_url(
    url: &StartupCoordinatorUrl,
    fileurlconnection: &Arc<dyn peerconnections_fileurlconnection::FileUrlConnection>,
    sftpurlconnection: &Arc<dyn peerconnections_sftpurlconnection::SftpUrlConnection>,
    global_connection: StartupCoordinatorGlobalSettings,
    run_mode: StartupCoordinatorRunMode,
    local_environment: &StartupCoordinatorLocalEnvironment,
) -> Result<StartupCoordinatorConnection, String> {
    match &url.location {
        StartupCoordinatorUrlLocation::File(file_url) => fileurlconnection
            .establish_file_url(peerconnections_fileurlconnection::FileUrlConnectionRequest {
                local_peer_root_path: file_url.local_peer_root_path.clone(),
                run_mode: file_run_mode(run_mode),
                timeout_conn_seconds: url
                    .connection
                    .timeout_conn_seconds
                    .unwrap_or(global_connection.timeout_conn_seconds),
                timeout_idle_seconds: url
                    .connection
                    .timeout_idle_seconds
                    .unwrap_or(global_connection.timeout_idle_seconds),
            })
            .map(|handle| {
                StartupCoordinatorConnection::File(StartupCoordinatorFileConnection {
                    local_peer_root_path: handle.local_peer_root_path,
                })
            })
            .map_err(|failure| failure.detail),
        StartupCoordinatorUrlLocation::Sftp(sftp_url) => sftpurlconnection
            .establish_sftp_url(peerconnections_sftpurlconnection::SftpUrlConnectionRequest {
                endpoint: peerconnections_sftpurlconnection::SftpUrlConnectionEndpoint {
                    host: sftp_url.host.clone(),
                    port: sftp_url.port,
                    username: sftp_url.username.clone(),
                },
                remote_peer_root_path: sftp_url.absolute_path.clone(),
                inline_password: sftp_url.password.clone(),
                url_timeout_conn_seconds: url.connection.timeout_conn_seconds,
                global_timeout_conn_seconds: global_connection.timeout_conn_seconds,
                run_mode: sftp_run_mode(run_mode),
                home_directory: local_environment.home_directory.clone(),
                known_hosts: peerconnections_sftpurlconnection::SftpUrlConnectionKnownHosts::Path(
                    local_environment.known_hosts_path.clone(),
                ),
                ssh_agent_socket: local_environment.ssh_agent_socket.clone(),
            })
            .map(|established| {
                StartupCoordinatorConnection::Sftp(StartupCoordinatorSftpConnection {
                    host: established.endpoint.host,
                    username: established.endpoint.username,
                    port: established.endpoint.port,
                    absolute_path: established.remote_peer_root_path,
                    timeout_conn_seconds: established.effective_timeout_conn_seconds,
                    timeout_idle_seconds: url
                        .connection
                        .timeout_idle_seconds
                        .unwrap_or(global_connection.timeout_idle_seconds),
                })
            })
            .map_err(|failure| format!("{:?}", failure.reason)),
    }
}

fn file_run_mode(
    run_mode: StartupCoordinatorRunMode,
) -> peerconnections_fileurlconnection::FileUrlConnectionRunMode {
    match run_mode {
        StartupCoordinatorRunMode::Normal => {
            peerconnections_fileurlconnection::FileUrlConnectionRunMode::Normal
        }
        StartupCoordinatorRunMode::DryRun => {
            peerconnections_fileurlconnection::FileUrlConnectionRunMode::DryRun
        }
    }
}

fn sftp_run_mode(
    run_mode: StartupCoordinatorRunMode,
) -> peerconnections_sftpurlconnection::SftpUrlConnectionRunMode {
    match run_mode {
        StartupCoordinatorRunMode::Normal => {
            peerconnections_sftpurlconnection::SftpUrlConnectionRunMode::Normal
        }
        StartupCoordinatorRunMode::DryRun => {
            peerconnections_sftpurlconnection::SftpUrlConnectionRunMode::DryRun
        }
    }
}
