use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerIdentityTarget {
    Local(LocalPeerIdentityTarget),
    Sftp(SftpPeerIdentityTarget),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalPeerIdentityTarget {
    pub path_or_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpPeerIdentityTarget {
    pub host: String,
    pub username: Option<String>,
    pub port: u16,
    pub absolute_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerIdentityNormalizationError {
    pub target: PeerIdentityTarget,
    pub message: String,
}

pub trait PeerIdentityNormalizer: Send + Sync {
    /// Normalizes one already-accepted peer target into the only URL identity
    /// form callers may use for equality, duplicate detection, snapshot lookup,
    /// or any other peer identity lookup.
    ///
    /// Local path targets become `file://` URL identities. Bare paths and
    /// Windows drive paths are treated as local paths, and relative local paths
    /// are resolved against the supplied process current working directory
    /// before the identity URL is built. For every peer URL identity, the
    /// scheme is lowercase and any hostname is lowercase. SFTP port 22 is
    /// removed from identity, while every non-default SFTP port is preserved.
    /// Peer URL paths collapse consecutive slashes into one slash, remove a
    /// trailing slash, decode percent-encoded unreserved path characters, and
    /// leave percent-encoded reserved path characters encoded. Query strings
    /// are stripped from the returned identity. SFTP targets with no username
    /// use the supplied operating-system username, and explicit SFTP usernames
    /// are preserved unchanged.
    ///
    /// The operation reports only normalization failures that prevent forming
    /// a peer URL identity from an already-accepted target, such as an absolute
    /// local path that cannot be represented as a `file://` URL. It does not
    /// parse command-line operands, validate peer roles, validate fallback
    /// groups, parse URL timeout settings, parse inline SFTP passwords, format
    /// command validation errors, write stdout, or write stderr. Repeated calls
    /// with the same inputs return the same identity or the same error.
    fn normalize_peer_identity(
        &self,
        target: PeerIdentityTarget,
        current_working_directory: PathBuf,
        current_os_username: String,
    ) -> Result<String, PeerIdentityNormalizationError>;
}
