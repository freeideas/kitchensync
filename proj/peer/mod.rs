use std::collections::HashMap;
use std::env;
use std::fmt;
use std::path::{Path, PathBuf};

pub use crate::{EffectivePeerRole, PeerSession};

use crate::{
    DiagnosticEvent, DiagnosticSink, PeerId, PeerRole, PeerSpec, PeerUrl, RunConfig,
    TransportFactory, TransportHandle, TransportRootMode, TransportTimeouts,
};

const SUMMARY: &str = "peer: startup peer reachability, fallback selection, identity normalization, and effective role resolution.";

#[derive(Clone)]
pub struct PendingPeerSession {
    pub id: PeerId,
    pub invocation_index: usize,
    pub normalized_identity: PeerUrl,
    pub selected_url: PeerUrl,
    pub declared_role: PeerRole,
    pub transport: TransportHandle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotExistence {
    pub peer_id: PeerId,
    pub existed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerStartupError {
    TooFewReachablePeers,
    DeclaredCanonUnreachable { peer_id: PeerId },
    FirstSyncNeedsCanon,
    NoContributingPeerReachable,
}

impl PeerStartupError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::TooFewReachablePeers => "At least two peers must be reachable",
            Self::DeclaredCanonUnreachable { .. } => "Declared canon peer is unreachable",
            Self::FirstSyncNeedsCanon => "First sync? Mark the authoritative peer with a leading +",
            Self::NoContributingPeerReachable => {
                "No contributing peer reachable - cannot make sync decisions"
            }
        }
    }
}

impl fmt::Display for PeerStartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for PeerStartupError {}

pub fn summary() -> &'static str {
    SUMMARY
}

pub async fn connect_peers(
    run_config: &RunConfig,
    peer_specs: &[PeerSpec],
    transport_factory: &dyn TransportFactory,
    diagnostics: &dyn DiagnosticSink,
) -> Result<Vec<PendingPeerSession>, PeerStartupError> {
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let current_user = current_user();
    let attempted = std::thread::scope(|scope| {
        let handles = peer_specs
            .iter()
            .enumerate()
            .map(|(invocation_index, spec)| {
                let current_dir = &current_dir;
                let current_user = &current_user;
                (
                    invocation_index,
                    scope.spawn(move || {
                        let peer_id = invocation_index as PeerId;
                        connect_one_peer(
                            peer_id,
                            invocation_index,
                            spec,
                            run_config,
                            transport_factory,
                            current_dir,
                            current_user,
                        )
                    }),
                )
            })
            .collect::<Vec<_>>();

        let mut attempted = handles
            .into_iter()
            .map(|(invocation_index, handle)| {
                let session = handle.join().unwrap_or(None);
                (invocation_index, session)
            })
            .collect::<Vec<_>>();
        attempted.sort_by_key(|(invocation_index, _)| *invocation_index);
        attempted
    });

    let mut pending = Vec::new();
    let mut unreachable_declared_canon = None;

    for (invocation_index, session) in attempted {
        let spec = &peer_specs[invocation_index];
        let peer_id = invocation_index as PeerId;
        match session {
            Some(session) => pending.push(session),
            None => {
                if spec.role == PeerRole::Canon {
                    unreachable_declared_canon = Some(peer_id);
                }
                diagnostics.publish(DiagnosticEvent::Error {
                    message: format!(
                        "Peer {} is unreachable; all configured URL candidates failed",
                        peer_id
                    ),
                });
            }
        }
    }

    if let Some(peer_id) = unreachable_declared_canon {
        return Err(PeerStartupError::DeclaredCanonUnreachable { peer_id });
    }

    if pending.len() < 2 {
        return Err(PeerStartupError::TooFewReachablePeers);
    }

    Ok(pending)
}

pub fn resolve_roles(
    pending_sessions: Vec<PendingPeerSession>,
    snapshot_existence: &[SnapshotExistence],
) -> Result<Vec<PeerSession>, PeerStartupError> {
    if pending_sessions.len() < 2 {
        return Err(PeerStartupError::TooFewReachablePeers);
    }

    let existence_by_peer = snapshot_existence
        .iter()
        .map(|existence| (existence.peer_id, existence.existed))
        .collect::<HashMap<_, _>>();

    let any_declared_canon = pending_sessions
        .iter()
        .any(|session| session.declared_role == PeerRole::Canon);
    let any_snapshot = pending_sessions
        .iter()
        .any(|session| existence_by_peer.get(&session.id).copied().unwrap_or(false));

    if !any_snapshot && !any_declared_canon {
        return Err(PeerStartupError::FirstSyncNeedsCanon);
    }

    let mut any_contributing = false;
    let sessions = pending_sessions
        .into_iter()
        .map(|pending| {
            let had_startup_snapshot = existence_by_peer.get(&pending.id).copied().unwrap_or(false);
            let effective_role = match pending.declared_role {
                PeerRole::Canon => EffectivePeerRole::Canon,
                PeerRole::Subordinate => EffectivePeerRole::Subordinate,
                PeerRole::Normal if had_startup_snapshot => EffectivePeerRole::Contributing,
                PeerRole::Normal => EffectivePeerRole::Subordinate,
            };

            if matches!(
                effective_role,
                EffectivePeerRole::Canon | EffectivePeerRole::Contributing
            ) {
                any_contributing = true;
            }

            PeerSession {
                id: pending.id,
                invocation_index: pending.invocation_index,
                normalized_identity: pending.normalized_identity,
                selected_url: pending.selected_url,
                declared_role: pending.declared_role,
                effective_role,
                transport: pending.transport,
                had_startup_snapshot,
            }
        })
        .collect::<Vec<_>>();

    if !any_contributing {
        return Err(PeerStartupError::NoContributingPeerReachable);
    }

    Ok(sessions)
}

fn connect_one_peer(
    peer_id: PeerId,
    invocation_index: usize,
    spec: &PeerSpec,
    run_config: &RunConfig,
    transport_factory: &dyn TransportFactory,
    current_dir: &Path,
    current_user: &str,
) -> Option<PendingPeerSession> {
    for url in &spec.urls {
        let selected_url = normalize_selected_url(url, current_dir, current_user);
        let normalized_identity = normalized_identity(&selected_url);
        let timeouts = TransportTimeouts {
            timeout_conn: selected_url.timeout_conn.unwrap_or(run_config.timeout_conn),
            timeout_idle: selected_url.timeout_idle.unwrap_or(run_config.timeout_idle),
        };
        let root_mode = if run_config.dry_run {
            TransportRootMode::RequireExisting
        } else {
            TransportRootMode::CreateMissing
        };

        if let Ok(transport) = transport_factory.connect(&selected_url, timeouts, root_mode) {
            return Some(PendingPeerSession {
                id: peer_id,
                invocation_index,
                normalized_identity,
                selected_url,
                declared_role: spec.role,
                transport,
            });
        }
    }

    None
}

fn normalize_selected_url(url: &PeerUrl, current_dir: &Path, current_user: &str) -> PeerUrl {
    let mut normalized = url.clone();
    let original_path = normalized.path.clone();
    let (path_without_query, query) = split_path_query(&original_path);
    normalized.scheme = normalized.scheme.to_ascii_lowercase();
    normalized.path = percent_decode_unreserved(&collapse_path_slashes(path_without_query));

    if normalized.scheme == "sftp" {
        normalized.host = normalized
            .host
            .as_ref()
            .map(|host| percent_decode_unreserved(host).to_ascii_lowercase());
        normalized.username = normalized
            .username
            .as_ref()
            .map(|username| percent_decode_unreserved(username));
        normalized.password = normalized
            .password
            .as_ref()
            .map(|password| percent_decode_unreserved(password));
        if normalized.port == Some(22) {
            normalized.port = None;
        }
        if normalized.username.is_none() {
            normalized.username = Some(current_user.to_string());
        }
    } else {
        normalized.scheme = "file".to_string();
        normalized.host = None;
        normalized.port = None;
        normalized.username = None;
        normalized.password = None;
        normalized.path = absolute_file_path(current_dir, &normalized.path);
    }

    normalized.path = trim_trailing_path_slash(&normalized.path);
    if let Some(query) = query {
        normalized.path.push('?');
        normalized.path.push_str(query);
    }
    normalized.identity = identity_string(&normalized);
    normalized
}

fn normalized_identity(url: &PeerUrl) -> PeerUrl {
    let mut identity = url.clone();
    identity.password = None;
    identity.timeout_conn = None;
    identity.timeout_idle = None;
    identity.path = strip_query_from_path(&identity.path).to_string();
    identity.identity = identity_string(&identity);
    identity
}

fn strip_query_from_path(path: &str) -> &str {
    split_path_query(path).0
}

fn split_path_query(path: &str) -> (&str, Option<&str>) {
    path.split_once('?')
        .map_or((path, None), |(without_query, query)| {
            (without_query, Some(query))
        })
}

fn absolute_file_path(current_dir: &Path, path: &str) -> String {
    let path = strip_file_url_drive_prefix(path);
    let candidate = PathBuf::from(path);
    let absolute = if candidate.is_absolute() || is_windows_drive_path(path) {
        candidate
    } else {
        current_dir.join(candidate)
    };
    normalize_file_identity_path(&absolute.to_string_lossy().replace('\\', "/"))
}

fn strip_file_url_drive_prefix(path: &str) -> &str {
    if path.len() >= 4 {
        let bytes = path.as_bytes();
        if bytes[0] == b'/'
            && bytes[2] == b':'
            && bytes[3] == b'/'
            && bytes[1].is_ascii_alphabetic()
        {
            return &path[1..];
        }
    }
    path
}

fn is_windows_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn normalize_file_identity_path(path: &str) -> String {
    let collapsed = collapse_path_slashes(path);
    let trimmed = trim_trailing_path_slash(&collapsed);
    let (prefix, rest) = file_path_prefix(&trimmed);
    let mut segments = Vec::new();

    for segment in rest.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            value => segments.push(value),
        }
    }

    let mut output = String::from(prefix);
    if !segments.is_empty() {
        if !output.is_empty() && !output.ends_with('/') {
            output.push('/');
        }
        output.push_str(&segments.join("/"));
    }

    if output.is_empty() {
        ".".to_string()
    } else {
        output
    }
}

fn file_path_prefix(path: &str) -> (&str, &str) {
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        return (&path[..3], &path[3..]);
    }
    if let Some(rest) = path.strip_prefix('/') {
        return ("/", rest);
    }
    ("", path)
}

fn collapse_path_slashes(path: &str) -> String {
    let mut collapsed = String::with_capacity(path.len());
    let mut previous_was_slash = false;
    for ch in path.chars() {
        if ch == '/' {
            if !previous_was_slash {
                collapsed.push(ch);
            }
            previous_was_slash = true;
        } else {
            collapsed.push(ch);
            previous_was_slash = false;
        }
    }
    collapsed
}

fn trim_trailing_path_slash(path: &str) -> String {
    if path == "/" || is_windows_drive_root(path) {
        path.to_string()
    } else if path.len() > 1 {
        path.trim_end_matches('/').to_string()
    } else {
        path.to_string()
    }
}

fn percent_decode_unreserved(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                let decoded = hi * 16 + lo;
                if is_unreserved(decoded) {
                    output.push(decoded as char);
                    index += 3;
                    continue;
                }
            }
        }
        let ch = value[index..].chars().next().expect("valid utf-8 boundary");
        output.push(ch);
        index += ch.len_utf8();
    }
    output
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn is_unreserved(value: u8) -> bool {
    matches!(
        value,
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~'
    )
}

fn is_windows_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

fn identity_string(url: &PeerUrl) -> String {
    match url.scheme.as_str() {
        "sftp" => {
            let mut identity = String::from("sftp://");
            if let Some(username) = &url.username {
                identity.push_str(username);
                identity.push('@');
            }
            if let Some(host) = &url.host {
                identity.push_str(host);
            }
            if let Some(port) = url.port {
                identity.push(':');
                identity.push_str(&port.to_string());
            }
            identity.push_str(&url.path);
            identity
        }
        _ => format!("file://{}", url.path),
    }
}

fn current_user() -> String {
    env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_resolution_requires_canon_for_first_sync() {
        let result = resolve_roles(Vec::new(), &[]);
        assert!(matches!(
            result,
            Err(PeerStartupError::TooFewReachablePeers)
        ));
    }

    #[test]
    fn percent_decodes_only_unreserved_characters() {
        assert_eq!(percent_decode_unreserved("a%2Fb%7Ec%20"), "a%2Fb~c%20");
    }
}
