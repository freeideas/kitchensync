use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpUrlConnectionRequest {
    pub endpoint: SftpUrlConnectionEndpoint,
    pub remote_peer_root_path: String,
    pub inline_password: Option<String>,
    pub url_timeout_conn_seconds: Option<u32>,
    pub global_timeout_conn_seconds: u32,
    pub run_mode: SftpUrlConnectionRunMode,
    pub home_directory: PathBuf,
    pub known_hosts: SftpUrlConnectionKnownHosts,
    pub ssh_agent_socket: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpUrlConnectionEndpoint {
    pub host: String,
    pub port: u16,
    pub username: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SftpUrlConnectionRunMode {
    Normal,
    DryRun,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SftpUrlConnectionKnownHosts {
    Path(PathBuf),
    Contents(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpUrlConnectionEstablished {
    pub endpoint: SftpUrlConnectionEndpoint,
    pub remote_peer_root_path: String,
    pub effective_timeout_conn_seconds: u32,
    pub connection: SftpUrlConnectionInfo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpUrlConnectionInfo {
    pub authenticated_with: SftpUrlConnectionCredentialSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpUrlConnectionFailure {
    pub endpoint: SftpUrlConnectionEndpoint,
    pub remote_peer_root_path: String,
    pub effective_timeout_conn_seconds: u32,
    pub reason: SftpUrlConnectionFailureReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SftpUrlConnectionFailureReason {
    ConnectionFailed { details: String },
    HandshakeTimedOut,
    HostKeyUntrusted(SftpUrlConnectionHostKeyFailure),
    AuthenticationExhausted {
        attempts: Vec<SftpUrlConnectionCredentialAttempt>,
    },
    RemoteRootPreparationFailed(SftpUrlConnectionRemoteRootFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SftpUrlConnectionHostKeyFailure {
    KnownHostsMissing,
    EntryMissing,
    EntryMismatched,
    KeyRejected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpUrlConnectionCredentialAttempt {
    pub source: SftpUrlConnectionCredentialSource,
    pub status: SftpUrlConnectionCredentialAttemptStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SftpUrlConnectionCredentialSource {
    InlinePassword,
    SshAgent,
    IdentityFileEd25519,
    IdentityFileEcdsa,
    IdentityFileRsa,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SftpUrlConnectionCredentialAttemptStatus {
    Absent,
    Unavailable { details: String },
    Rejected { details: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpUrlConnectionRemoteRootFailure {
    pub kind: SftpUrlConnectionRemoteRootFailureKind,
    pub details: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SftpUrlConnectionRemoteRootFailureKind {
    MissingInDryRun,
    CreationFailed,
}

pub trait SftpUrlConnection: Send + Sync {
    /// Attempts to establish exactly one already-parsed `sftp://` URL and
    /// returns either a connected result for that URL or a structured failure
    /// for that URL.
    ///
    /// The effective SSH handshake timeout is the request's
    /// `url_timeout_conn_seconds` value when present, or
    /// `global_timeout_conn_seconds` otherwise. If that timeout expires before
    /// the SSH handshake completes, only this URL fails.
    ///
    /// The server host key must be trusted before the connection is accepted.
    /// Trust requires a matching `known_hosts` entry for the contacted host and
    /// port. A missing known-hosts file, missing entry, mismatched entry, or
    /// rejected key fails only this URL.
    ///
    /// Authentication tries credential sources in this exact order: inline URL
    /// password, SSH agent, `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa`, then
    /// `~/.ssh/id_rsa`. An absent, unavailable, or rejected source does not
    /// stop the fallback chain; authentication fails only after every listed
    /// source has been tried or skipped.
    ///
    /// After connection and authentication succeed, normal mode creates the
    /// remote peer root directory and any missing parents through SFTP before
    /// accepting the URL. If that creation fails, only this URL fails. Dry-run
    /// mode must not create remote directories; a missing remote root fails
    /// only this URL.
    ///
    /// This operation does not parse command-line text, normalize URL
    /// identity, insert default usernames, decode URL fields, choose among a
    /// peer's fallback URLs, decide startup reachability, format final user
    /// output, or perform later sync operations beyond startup root
    /// preparation.
    fn establish_sftp_url(
        &self,
        request: SftpUrlConnectionRequest,
    ) -> Result<SftpUrlConnectionEstablished, SftpUrlConnectionFailure>;
}
