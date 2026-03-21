use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

use crate::config::{Config, PeerEntry, UrlEntry};
use crate::filesystem::PeerFs;
use crate::local_fs::LocalFs;
use crate::sftp_fs::SftpFs;

/// A connection pool for a single URL.
pub struct UrlPool {
    pub url: String,
    pub semaphore: Arc<Semaphore>,
    pub max_connections: u32,
    pub connection_timeout: u32,
}

/// A peer with its active filesystem connection and pool.
pub struct ConnectedPeer {
    pub peer_id: i64,
    pub name: String,
    pub active_url: String,
    pub fs: Arc<dyn PeerFs>,
    pub pool: Arc<UrlPool>,
    pub canon: bool,
}

/// Parse an SFTP URL into components.
fn parse_sftp_url(url: &str) -> Option<(String, Option<String>, String, u16, String)> {
    // sftp://[user[:password]@]host[:port]/path
    let rest = url.strip_prefix("sftp://")?;
    let (userinfo, hostpath) = if let Some(at_pos) = {
        let path_start = rest.find('/').unwrap_or(rest.len());
        rest[..path_start].rfind('@')
    } {
        (&rest[..at_pos], &rest[at_pos + 1..])
    } else {
        ("", rest)
    };

    let (host_port, path) = if let Some(slash_pos) = hostpath.find('/') {
        (&hostpath[..slash_pos], &hostpath[slash_pos..])
    } else {
        return None;
    };

    let (host, port) = if let Some(colon_pos) = host_port.rfind(':') {
        let potential_port = &host_port[colon_pos + 1..];
        if let Ok(p) = potential_port.parse::<u16>() {
            (&host_port[..colon_pos], p)
        } else {
            (host_port, 22u16)
        }
    } else {
        (host_port, 22u16)
    };

    let (username, password) = if userinfo.is_empty() {
        (String::new(), None)
    } else if let Some(colon) = userinfo.find(':') {
        (
            userinfo[..colon].to_string(),
            Some(percent_decode(&userinfo[colon + 1..])),
        )
    } else {
        (userinfo.to_string(), None)
    };

    Some((username, password, host.to_string(), port, path.to_string()))
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .to_string()
}

/// Connect to a peer, trying URLs in order.
pub async fn connect_peer(
    peer: &PeerEntry,
    peer_id: i64,
    global_config: &Config,
    runtime_canon: bool,
) -> Result<ConnectedPeer, String> {
    for url_entry in &peer.urls {
        let url = url_entry.url_str();
        let max_conn = url_entry
            .max_connections()
            .unwrap_or_else(|| global_config.max_connections());
        let timeout = url_entry
            .connection_timeout()
            .unwrap_or_else(|| global_config.connection_timeout());

        let norm_url = crate::url_normalize::normalize_url(url).unwrap_or_else(|_| url.to_string());

        let fs_result: Result<Arc<dyn PeerFs>, String> = if norm_url.starts_with("file://") {
            let path = &norm_url["file://".len()..];
            let path = if cfg!(windows) && path.starts_with('/') {
                &path[1..] // Remove leading slash for Windows paths like /c:/foo
            } else {
                path
            };
            let pb = std::path::PathBuf::from(path);
            if !pb.exists() {
                if let Err(e) = std::fs::create_dir_all(&pb) {
                    eprintln!("Warning: cannot create local path {}: {}", path, e);
                    continue;
                }
            }
            Ok(Arc::new(LocalFs::new(pb)))
        } else if norm_url.starts_with("sftp://") {
            match parse_sftp_url(&norm_url) {
                Some((user, pass, host, port, path)) => {
                    match SftpFs::connect(&host, port, &user, pass.as_deref(), timeout, &path) {
                        Ok(fs) => {
                            // Ensure root path exists on remote
                            if let Err(e) = fs.ensure_root_dir() {
                                eprintln!("Warning: cannot create remote path on {}: {}", url, e);
                                continue;
                            }
                            Ok(Arc::new(fs))
                        }
                        Err(e) => {
                            eprintln!("Warning: cannot connect to {}: {}", url, e);
                            continue;
                        }
                    }
                }
                None => {
                    eprintln!("Warning: cannot parse SFTP URL: {}", url);
                    continue;
                }
            }
        } else {
            eprintln!("Warning: unsupported URL scheme: {}", url);
            continue;
        };

        match fs_result {
            Ok(fs) => {
                let pool = Arc::new(UrlPool {
                    url: norm_url.clone(),
                    semaphore: Arc::new(Semaphore::new(max_conn as usize)),
                    max_connections: max_conn,
                    connection_timeout: timeout,
                });
                return Ok(ConnectedPeer {
                    peer_id,
                    name: peer.name.clone(),
                    active_url: norm_url,
                    fs,
                    pool,
                    canon: peer.canon || runtime_canon,
                });
            }
            Err(e) => {
                eprintln!("Warning: cannot connect to {}: {}", url, e);
                continue;
            }
        }
    }

    Err(format!("all URLs failed for peer '{}'", peer.name))
}
