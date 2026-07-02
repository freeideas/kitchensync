#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerArgumentParseResult {
    Parsed(Vec<PeerArgumentPeer>),
    ValidationFailure(PeerArgumentValidationReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerArgumentPeer {
    pub role: PeerArgumentPeerRole,
    pub fallback_targets: Vec<PeerArgumentTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerArgumentPeerRole {
    Canon,
    Subordinate,
    Normal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerArgumentTarget {
    pub location: PeerArgumentLocation,
    pub connection: PeerArgumentUrlConnectionSettings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerArgumentLocation {
    Local(PeerArgumentLocalTarget),
    Sftp(PeerArgumentSftpTarget),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerArgumentLocalTarget {
    pub path_or_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerArgumentSftpTarget {
    pub host: String,
    pub username: String,
    pub password: Option<String>,
    pub port: u16,
    pub absolute_path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerArgumentUrlConnectionSettings {
    pub timeout_conn_seconds: u32,
    pub timeout_idle_seconds: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerArgumentValidationReason {
    TooFewPeerOperands,
    MoreThanOneCanonPeer,
    UnsupportedPeerUrlForm,
    UnsupportedQueryParameter,
    InvalidUrlTimeoutValue,
}

pub trait PeerArgumentParser: Send + Sync {
    /// Parses the ordered peer operands left after global option parsing into
    /// validated peer records.
    ///
    /// A successful parse preserves peer operand order, preserves fallback
    /// target order inside each bracketed peer, accepts two or more peers, and
    /// accepts at most one canon peer. Each successful peer has exactly one
    /// role: canon for one leading `+`, subordinate for one leading `-`, or
    /// normal for no marker. A leading role marker before a bracketed operand
    /// applies to the whole fallback peer; role marker characters inside the
    /// brackets remain part of the member text and are not parsed as
    /// per-member roles. Additional peer operands after the first two are
    /// accepted.
    ///
    /// Bare paths with no URL scheme, including Unix absolute paths, Windows
    /// drive paths, and relative paths, are recorded as local targets. `file://`
    /// URLs are recorded as local targets. `sftp://` URLs are recorded as SFTP
    /// targets with host, username, optional inline password, SSH port, remote
    /// absolute path, and URL-level connection settings. Missing SFTP usernames
    /// use the supplied current operating-system username, missing SFTP ports
    /// use port 22, and percent-encoded `@` and `:` characters in an inline
    /// password are decoded as password characters. The parser does not
    /// resolve local paths, create file URL identities, normalize peer
    /// identities, lowercase schemes or hosts for identity, strip query strings
    /// for identity, connect to peers, authenticate, choose fallback targets,
    /// list files, create directories, or make sync decisions.
    ///
    /// The only accepted URL query parameters are `timeout-conn` and
    /// `timeout-idle`. Each timeout value must be a positive integer number of
    /// seconds. A URL-level timeout overrides only the matching global timeout
    /// for that URL; an absent timeout query parameter keeps the matching
    /// supplied global timeout. `max-copies` and every query parameter other
    /// than `timeout-conn` and `timeout-idle` are validation failures.
    ///
    /// Validation returns exactly one reason identifying too few peer operands,
    /// more than one canon peer, an unsupported peer URL form, an unsupported
    /// query parameter, or an invalid URL timeout value. Repeated calls with
    /// the same inputs return the same result and do not emit output.
    fn parse_peer_arguments(
        &self,
        peer_operands: Vec<String>,
        global_timeout_conn_seconds: u32,
        global_timeout_idle_seconds: u32,
        current_os_username: String,
    ) -> PeerArgumentParseResult;
}
