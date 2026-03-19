use percent_encoding::percent_decode_str;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
pub struct RawConfig {
    pub database: Option<String>,
    #[serde(rename = "connection-timeout")]
    pub connection_timeout: Option<u64>,
    pub workers: Option<usize>,
    #[serde(rename = "xfer-cleanup-days")]
    pub xfer_cleanup_days: Option<u64>,
    #[serde(rename = "back-retention-days")]
    pub back_retention_days: Option<u64>,
    #[serde(rename = "tombstone-retention-days")]
    pub tombstone_retention_days: Option<u64>,
    #[serde(rename = "log-retention-days")]
    pub log_retention_days: Option<u64>,
    pub peers: HashMap<String, RawPeerConfig>,
}

#[derive(Deserialize)]
pub struct RawPeerConfig {
    pub urls: Vec<String>,
}

pub struct Config {
    pub config_file_path: PathBuf,
    pub config_dir: PathBuf,
    pub database_path: PathBuf,
    pub connection_timeout: u64,
    pub workers: usize,
    pub xfer_cleanup_days: u64,
    pub back_retention_days: u64,
    pub tombstone_retention_days: u64,
    pub log_retention_days: u64,
    pub peers: HashMap<String, PeerConfig>,
}

pub struct PeerConfig {
    pub name: String,
    pub urls: Vec<PeerUrl>,
}

pub struct PeerUrl {
    pub scheme: String,
    pub user: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub path: PathBuf,
}

pub fn resolve_config_path(arg: &str) -> Result<PathBuf, String> {
    let p = Path::new(arg);
    if p.extension().map_or(false, |e| e == "json") && p.is_file() {
        return Ok(p.canonicalize().map_err(|e| format!("Cannot resolve {}: {}", arg, e))?);
    }
    if p.is_dir() {
        let name = p.file_name().unwrap_or_default();
        if name == ".kitchensync" {
            let candidate = p.join("kitchensync-conf.json");
            if candidate.is_file() {
                return Ok(candidate.canonicalize().map_err(|e| e.to_string())?);
            }
            return Err(format!("Config file not found: {}", candidate.display()));
        }
        let candidate = p.join(".kitchensync").join("kitchensync-conf.json");
        if candidate.is_file() {
            return Ok(candidate.canonicalize().map_err(|e| e.to_string())?);
        }
        return Err(format!("Config file not found: {}", candidate.display()));
    }
    if p.is_file() {
        return Ok(p.canonicalize().map_err(|e| format!("Cannot resolve {}: {}", arg, e))?);
    }
    Err(format!("Path does not exist: {}", arg))
}

pub fn load_config(config_file: &Path) -> Result<Config, String> {
    let content = std::fs::read_to_string(config_file)
        .map_err(|e| format!("Cannot read {}: {}", config_file.display(), e))?;
    let raw: RawConfig =
        json5::from_str(&content).map_err(|e| format!("Invalid config: {}", e))?;

    let config_dir = config_file.parent().unwrap().to_path_buf();
    let in_kitchensync_dir = config_dir
        .file_name()
        .map_or(false, |n| n == ".kitchensync");

    let db_rel = raw
        .database
        .unwrap_or_else(|| "kitchensync.db".to_string());
    let database_path = if Path::new(&db_rel).is_absolute() {
        PathBuf::from(&db_rel)
    } else {
        config_dir.join(&db_rel)
    };

    let peer_name_re = regex_lite::Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9_-]*$").unwrap();

    let mut peers = HashMap::new();
    for (name, raw_peer) in &raw.peers {
        if name.len() > 64 || !peer_name_re.is_match(name) {
            return Err(format!("Invalid peer name: {}", name));
        }
        let mut urls = Vec::new();
        for raw_url in &raw_peer.urls {
            let parsed = parse_peer_url(raw_url, &config_dir, in_kitchensync_dir)?;
            urls.push(parsed);
        }
        peers.insert(
            name.clone(),
            PeerConfig {
                name: name.clone(),
                urls,
            },
        );
    }

    Ok(Config {
        config_file_path: config_file.to_path_buf(),
        config_dir,
        database_path,
        connection_timeout: raw.connection_timeout.unwrap_or(30),
        workers: raw.workers.unwrap_or(10),
        xfer_cleanup_days: raw.xfer_cleanup_days.unwrap_or(2),
        back_retention_days: raw.back_retention_days.unwrap_or(90),
        tombstone_retention_days: raw.tombstone_retention_days.unwrap_or(180),
        log_retention_days: raw.log_retention_days.unwrap_or(32),
        peers,
    })
}

fn parse_peer_url(
    raw: &str,
    config_dir: &Path,
    in_kitchensync_dir: bool,
) -> Result<PeerUrl, String> {
    // Handle file://./relative specially since url crate won't parse it correctly
    if raw.starts_with("file://./") || raw.starts_with("file://../") {
        let rel = &raw["file://".len()..];
        let mut base = config_dir.to_path_buf();
        if in_kitchensync_dir {
            base = base.parent().unwrap_or(config_dir).to_path_buf();
        }
        let path = base.join(rel);
        return Ok(PeerUrl {
            scheme: "file".to_string(),
            user: None,
            password: None,
            host: None,
            port: None,
            path,
        });
    }

    let parsed = url::Url::parse(raw).map_err(|e| format!("Invalid URL {}: {}", raw, e))?;
    let scheme = parsed.scheme().to_string();

    match scheme.as_str() {
        "file" => {
            let decoded = percent_decode_str(parsed.path()).decode_utf8_lossy().to_string();
            let path = PathBuf::from(&decoded);
            Ok(PeerUrl {
                scheme,
                user: None,
                password: None,
                host: None,
                port: None,
                path,
            })
        }
        "sftp" => {
            let host = parsed
                .host_str()
                .ok_or_else(|| format!("No host in URL: {}", raw))?
                .to_string();
            let port = parsed.port();
            let user = if parsed.username().is_empty() {
                None
            } else {
                Some(
                    percent_decode_str(parsed.username())
                        .decode_utf8_lossy()
                        .to_string(),
                )
            };
            let password = parsed.password().map(|p| {
                percent_decode_str(p).decode_utf8_lossy().to_string()
            });
            let path = PathBuf::from(
                percent_decode_str(parsed.path())
                    .decode_utf8_lossy()
                    .to_string(),
            );
            Ok(PeerUrl {
                scheme,
                user,
                password,
                host: Some(host),
                port,
                path,
            })
        }
        _ => Err(format!("Unsupported URL scheme: {}", scheme)),
    }
}
