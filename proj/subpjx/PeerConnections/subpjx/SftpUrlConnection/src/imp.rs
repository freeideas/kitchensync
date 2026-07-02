use crate::api::*;
use ssh2::{CheckResult, KnownHostFileKind, Session, Sftp};
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

struct SftpUrlConnectionImpl;

impl SftpUrlConnection for SftpUrlConnectionImpl {
    fn establish_sftp_url(
        &self,
        request: SftpUrlConnectionRequest,
    ) -> Result<SftpUrlConnectionEstablished, SftpUrlConnectionFailure> {
        let effective_timeout_conn_seconds = request
            .url_timeout_conn_seconds
            .unwrap_or(request.global_timeout_conn_seconds);
        let timeout = Duration::from_secs(u64::from(effective_timeout_conn_seconds));

        let session = connect(&request.endpoint, timeout).map_err(|reason| {
            failure(&request, effective_timeout_conn_seconds, reason)
        })?;

        verify_host_key(&session, &request).map_err(|reason| {
            failure(&request, effective_timeout_conn_seconds, reason)
        })?;

        let authenticated_with = authenticate(&session, &request).map_err(|attempts| {
            failure(
                &request,
                effective_timeout_conn_seconds,
                SftpUrlConnectionFailureReason::AuthenticationExhausted { attempts },
            )
        })?;

        let sftp = session.sftp().map_err(|error| {
            failure(
                &request,
                effective_timeout_conn_seconds,
                SftpUrlConnectionFailureReason::RemoteRootPreparationFailed(
                    SftpUrlConnectionRemoteRootFailure {
                        kind: SftpUrlConnectionRemoteRootFailureKind::CreationFailed,
                        details: error.to_string(),
                    },
                ),
            )
        })?;
        prepare_remote_root(&sftp, &request).map_err(|reason| {
            failure(&request, effective_timeout_conn_seconds, reason)
        })?;

        Ok(SftpUrlConnectionEstablished {
            endpoint: request.endpoint,
            remote_peer_root_path: request.remote_peer_root_path,
            effective_timeout_conn_seconds,
            connection: SftpUrlConnectionInfo { authenticated_with },
        })
    }
}

fn connect(
    endpoint: &SftpUrlConnectionEndpoint,
    timeout: Duration,
) -> Result<Session, SftpUrlConnectionFailureReason> {
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(connection_failed)?
        .next()
        .ok_or_else(|| {
            SftpUrlConnectionFailureReason::ConnectionFailed {
                details: "host did not resolve to an address".to_string(),
            }
        })?;
    let tcp = if timeout.is_zero() {
        TcpStream::connect(address)
    } else {
        TcpStream::connect_timeout(&address, timeout)
    }
    .map_err(|error| {
        if error.kind() == io::ErrorKind::TimedOut {
            SftpUrlConnectionFailureReason::HandshakeTimedOut
        } else {
            connection_failed(error)
        }
    })?;
    if !timeout.is_zero() {
        tcp.set_read_timeout(Some(timeout)).map_err(connection_failed)?;
        tcp.set_write_timeout(Some(timeout)).map_err(connection_failed)?;
    }

    let mut session = Session::new().map_err(connection_failed)?;
    session.set_tcp_stream(tcp);
    session.set_timeout(timeout.as_millis().min(u128::from(u32::MAX)) as u32);
    session.handshake().map_err(|error| {
        if error.to_string().to_ascii_lowercase().contains("timed out") {
            SftpUrlConnectionFailureReason::HandshakeTimedOut
        } else {
            SftpUrlConnectionFailureReason::ConnectionFailed {
                details: error.to_string(),
            }
        }
    })?;
    Ok(session)
}

fn verify_host_key(
    session: &Session,
    request: &SftpUrlConnectionRequest,
) -> Result<(), SftpUrlConnectionFailureReason> {
    let (raw_key, _) = session.host_key().ok_or(
        SftpUrlConnectionFailureReason::HostKeyUntrusted(
            SftpUrlConnectionHostKeyFailure::KeyRejected,
        ),
    )?;
    let mut known_hosts = session.known_hosts().map_err(|_| {
        SftpUrlConnectionFailureReason::HostKeyUntrusted(
            SftpUrlConnectionHostKeyFailure::KeyRejected,
        )
    })?;

    match &request.known_hosts {
        SftpUrlConnectionKnownHosts::Path(path) => {
            if !path.is_file() {
                return Err(SftpUrlConnectionFailureReason::HostKeyUntrusted(
                    SftpUrlConnectionHostKeyFailure::KnownHostsMissing,
                ));
            }
            known_hosts
                .read_file(path, KnownHostFileKind::OpenSSH)
                .map_err(|_| {
                    SftpUrlConnectionFailureReason::HostKeyUntrusted(
                        SftpUrlConnectionHostKeyFailure::KeyRejected,
                    )
                })?;
        }
        SftpUrlConnectionKnownHosts::Contents(contents) => {
            if contents.is_empty() {
                return Err(SftpUrlConnectionFailureReason::HostKeyUntrusted(
                    SftpUrlConnectionHostKeyFailure::EntryMissing,
                ));
            }
            for line in contents.lines().filter(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty() && !trimmed.starts_with('#')
            }) {
                known_hosts
                    .read_str(line, KnownHostFileKind::OpenSSH)
                    .map_err(|_| {
                        SftpUrlConnectionFailureReason::HostKeyUntrusted(
                            SftpUrlConnectionHostKeyFailure::KeyRejected,
                        )
                    })?;
            }
        }
    }

    match known_hosts.check_port(&request.endpoint.host, request.endpoint.port, raw_key) {
        CheckResult::Match => Ok(()),
        CheckResult::Mismatch => Err(SftpUrlConnectionFailureReason::HostKeyUntrusted(
            SftpUrlConnectionHostKeyFailure::EntryMismatched,
        )),
        CheckResult::NotFound => Err(SftpUrlConnectionFailureReason::HostKeyUntrusted(
            SftpUrlConnectionHostKeyFailure::EntryMissing,
        )),
        CheckResult::Failure => Err(SftpUrlConnectionFailureReason::HostKeyUntrusted(
            SftpUrlConnectionHostKeyFailure::KeyRejected,
        )),
    }
}

fn authenticate(
    session: &Session,
    request: &SftpUrlConnectionRequest,
) -> Result<SftpUrlConnectionCredentialSource, Vec<SftpUrlConnectionCredentialAttempt>> {
    let mut attempts = Vec::new();

    if let Some(password) = &request.inline_password {
        match session.userauth_password(&request.endpoint.username, password) {
            Ok(()) => return Ok(SftpUrlConnectionCredentialSource::InlinePassword),
            Err(error) => attempts.push(rejected(
                SftpUrlConnectionCredentialSource::InlinePassword,
                error,
            )),
        }
    } else {
        attempts.push(absent(SftpUrlConnectionCredentialSource::InlinePassword));
    }

    match authenticate_with_agent(session, request) {
        Ok(true) => return Ok(SftpUrlConnectionCredentialSource::SshAgent),
        Ok(false) => attempts.push(absent(SftpUrlConnectionCredentialSource::SshAgent)),
        Err(status) => attempts.push(SftpUrlConnectionCredentialAttempt {
            source: SftpUrlConnectionCredentialSource::SshAgent,
            status,
        }),
    }

    for (source, file_name) in [
        (
            SftpUrlConnectionCredentialSource::IdentityFileEd25519,
            "id_ed25519",
        ),
        (
            SftpUrlConnectionCredentialSource::IdentityFileEcdsa,
            "id_ecdsa",
        ),
        (SftpUrlConnectionCredentialSource::IdentityFileRsa, "id_rsa"),
    ] {
        let private_key = request.home_directory.join(".ssh").join(file_name);
        if !private_key.is_file() {
            attempts.push(absent(source));
            continue;
        }
        match session.userauth_pubkey_file(&request.endpoint.username, None, &private_key, None) {
            Ok(()) => return Ok(source),
            Err(error) => attempts.push(rejected(source, error)),
        }
    }

    Err(attempts)
}

fn authenticate_with_agent(
    session: &Session,
    request: &SftpUrlConnectionRequest,
) -> Result<bool, SftpUrlConnectionCredentialAttemptStatus> {
    let Some(socket) = &request.ssh_agent_socket else {
        return Ok(false);
    };
    if socket.is_empty() {
        return Ok(false);
    }

    let previous_socket = std::env::var_os("SSH_AUTH_SOCK");
    std::env::set_var("SSH_AUTH_SOCK", socket);
    let result = (|| {
        let mut agent = session.agent().map_err(unavailable)?;
        agent.connect().map_err(unavailable)?;
        agent.list_identities().map_err(unavailable)?;
        let identities = agent.identities().map_err(unavailable)?;
        if identities.is_empty() {
            let _ = agent.disconnect();
            return Ok(false);
        }

        let mut last_rejection = None;
        for identity in identities {
            match agent.userauth(&request.endpoint.username, &identity) {
                Ok(()) => {
                    let _ = agent.disconnect();
                    return Ok(true);
                }
                Err(error) => last_rejection = Some(error.to_string()),
            }
        }
        let _ = agent.disconnect();
        Err(SftpUrlConnectionCredentialAttemptStatus::Rejected {
            details: last_rejection.unwrap_or_else(|| "agent identity rejected".to_string()),
        })
    })();

    match previous_socket {
        Some(value) => std::env::set_var("SSH_AUTH_SOCK", value),
        None => std::env::remove_var("SSH_AUTH_SOCK"),
    }

    result
}

fn prepare_remote_root(
    sftp: &Sftp,
    request: &SftpUrlConnectionRequest,
) -> Result<(), SftpUrlConnectionFailureReason> {
    let remote_root = request.remote_peer_root_path.as_str();
    if sftp.stat(Path::new(remote_root)).is_ok() {
        return Ok(());
    }

    if request.run_mode == SftpUrlConnectionRunMode::DryRun {
        return Err(SftpUrlConnectionFailureReason::RemoteRootPreparationFailed(
            SftpUrlConnectionRemoteRootFailure {
                kind: SftpUrlConnectionRemoteRootFailureKind::MissingInDryRun,
                details: "remote peer root is missing".to_string(),
            },
        ));
    }

    for parent in remote_parent_paths(remote_root) {
        if sftp.stat(Path::new(&parent)).is_ok() {
            continue;
        }
        if let Err(error) = sftp.mkdir(Path::new(&parent), 0o755) {
            if sftp.stat(Path::new(&parent)).is_err() {
                return Err(SftpUrlConnectionFailureReason::RemoteRootPreparationFailed(
                    SftpUrlConnectionRemoteRootFailure {
                        kind: SftpUrlConnectionRemoteRootFailureKind::CreationFailed,
                        details: error.to_string(),
                    },
                ));
            }
        }
    }

    if let Err(error) = sftp.mkdir(Path::new(remote_root), 0o755) {
        if sftp.stat(Path::new(remote_root)).is_err() {
            return Err(SftpUrlConnectionFailureReason::RemoteRootPreparationFailed(
                SftpUrlConnectionRemoteRootFailure {
                    kind: SftpUrlConnectionRemoteRootFailureKind::CreationFailed,
                    details: error.to_string(),
                },
            ));
        }
    }

    sftp.stat(Path::new(remote_root)).map(|_| ()).map_err(|error| {
        SftpUrlConnectionFailureReason::RemoteRootPreparationFailed(
            SftpUrlConnectionRemoteRootFailure {
                kind: SftpUrlConnectionRemoteRootFailureKind::CreationFailed,
                details: error.to_string(),
            },
        )
    })
}

fn remote_parent_paths(remote_root: &str) -> Vec<String> {
    let absolute = remote_root.starts_with('/');
    let parts: Vec<&str> = remote_root.split('/').filter(|part| !part.is_empty()).collect();
    let mut parents = Vec::new();
    if parts.len() <= 1 {
        return parents;
    }

    let mut current = if absolute {
        String::from("/")
    } else {
        String::new()
    };
    for part in parts.iter().take(parts.len() - 1) {
        if !current.is_empty() && current != "/" {
            current.push('/');
        }
        current.push_str(part);
        parents.push(current.clone());
    }
    parents
}

fn failure(
    request: &SftpUrlConnectionRequest,
    effective_timeout_conn_seconds: u32,
    reason: SftpUrlConnectionFailureReason,
) -> SftpUrlConnectionFailure {
    SftpUrlConnectionFailure {
        endpoint: request.endpoint.clone(),
        remote_peer_root_path: request.remote_peer_root_path.clone(),
        effective_timeout_conn_seconds,
        reason,
    }
}

fn absent(source: SftpUrlConnectionCredentialSource) -> SftpUrlConnectionCredentialAttempt {
    SftpUrlConnectionCredentialAttempt {
        source,
        status: SftpUrlConnectionCredentialAttemptStatus::Absent,
    }
}

fn rejected(
    source: SftpUrlConnectionCredentialSource,
    error: ssh2::Error,
) -> SftpUrlConnectionCredentialAttempt {
    SftpUrlConnectionCredentialAttempt {
        source,
        status: SftpUrlConnectionCredentialAttemptStatus::Rejected {
            details: error.to_string(),
        },
    }
}

fn unavailable(error: ssh2::Error) -> SftpUrlConnectionCredentialAttemptStatus {
    SftpUrlConnectionCredentialAttemptStatus::Unavailable {
        details: error.to_string(),
    }
}

fn connection_failed(error: impl ToString) -> SftpUrlConnectionFailureReason {
    SftpUrlConnectionFailureReason::ConnectionFailed {
        details: error.to_string(),
    }
}

pub fn new() -> std::sync::Arc<dyn SftpUrlConnection> {
    Arc::new(SftpUrlConnectionImpl)
}
