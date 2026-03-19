use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub config_file_path: PathBuf,
    pub config_dir: PathBuf,
    pub database: String,
    pub connection_timeout: u64,
    pub max_connections: usize,
    pub xfer_cleanup_days: u64,
    pub back_retention_days: u64,
    pub tombstone_retention_days: u64,
    pub log_retention_days: u64,
    pub peers: HashMap<String, PeerConfig>,
}

#[derive(Debug, Clone)]
pub struct PeerConfig {
    pub name: String,
    pub urls: Vec<PeerUrl>,
}

#[derive(Debug, Clone)]
pub struct PeerUrl {
    pub scheme: UrlScheme,
    pub user: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: u16,
    pub path: String,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UrlScheme {
    File,
    Sftp,
}

/// Resolve config file path from the <config> argument.
/// 1. Path to a .json file -> use directly
/// 2. Path to a .kitchensync/ dir -> append kitchensync-conf.json
/// 3. Path to any other dir -> append .kitchensync/kitchensync-conf.json
pub fn resolve_config_path(arg: &str) -> Result<PathBuf, String> {
    let p = Path::new(arg);
    if p.extension().map(|e| e == "json").unwrap_or(false) && p.is_file() {
        return Ok(p.to_path_buf());
    }
    if p.is_dir() {
        if p.file_name().map(|n| n == ".kitchensync").unwrap_or(false) {
            let conf = p.join("kitchensync-conf.json");
            if conf.is_file() {
                return Ok(conf);
            }
            return Err(format!("Config file not found: {}", conf.display()));
        }
        let conf = p.join(".kitchensync").join("kitchensync-conf.json");
        if conf.is_file() {
            return Ok(conf);
        }
        return Err(format!("Config file not found: {}", conf.display()));
    }
    if p.is_file() {
        return Ok(p.to_path_buf());
    }
    Err(format!("Config path not found: {}", arg))
}

pub fn load_config(path: &Path) -> Result<Config, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read config file {}: {}", path.display(), e))?;
    let val: serde_json::Value = json5::from_str(&content)
        .map_err(|e| format!("Invalid JSON5 in {}: {}", path.display(), e))?;

    let config_dir = path.parent().unwrap().to_path_buf();

    // Check if config is inside .kitchensync/ directory
    let inside_kitchensync = config_dir
        .file_name()
        .map(|n| n == ".kitchensync")
        .unwrap_or(false);

    let database = val
        .get("database")
        .and_then(|v| v.as_str())
        .unwrap_or("kitchensync.db")
        .to_string();

    let connection_timeout = val
        .get("connection-timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);
    let max_connections = val
        .get("max-connections")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;
    let xfer_cleanup_days = val
        .get("xfer-cleanup-days")
        .and_then(|v| v.as_u64())
        .unwrap_or(2);
    let back_retention_days = val
        .get("back-retention-days")
        .and_then(|v| v.as_u64())
        .unwrap_or(90);
    let tombstone_retention_days = val
        .get("tombstone-retention-days")
        .and_then(|v| v.as_u64())
        .unwrap_or(180);
    let log_retention_days = val
        .get("log-retention-days")
        .and_then(|v| v.as_u64())
        .unwrap_or(32);

    let peers_val = val
        .get("peers")
        .ok_or("Config missing 'peers' section")?;
    let peers_obj = peers_val
        .as_object()
        .ok_or("'peers' must be an object")?;

    let mut peers = HashMap::new();
    let peer_name_re = regex_lite(r"^[a-zA-Z0-9][a-zA-Z0-9_-]*$");

    for (name, pval) in peers_obj {
        if name.len() > 64 {
            return Err(format!("Peer name '{}' exceeds 64 characters", name));
        }
        if !peer_name_re.is_match(name) {
            return Err(format!(
                "Invalid peer name '{}': must match [a-zA-Z0-9][a-zA-Z0-9_-]*",
                name
            ));
        }
        let urls_val = pval
            .get("urls")
            .ok_or(format!("Peer '{}' missing 'urls'", name))?;
        let urls_arr = urls_val
            .as_array()
            .ok_or(format!("Peer '{}' urls must be an array", name))?;
        if urls_arr.is_empty() {
            return Err(format!("Peer '{}' has no URLs", name));
        }
        let mut urls = Vec::new();
        for u in urls_arr {
            let raw = u
                .as_str()
                .ok_or(format!("Peer '{}' URL must be a string", name))?;
            let parsed = parse_url(raw, &config_dir, inside_kitchensync)?;
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

    if peers.len() < 2 {
        return Err("Config must define at least two peers".to_string());
    }

    Ok(Config {
        config_file_path: path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
        config_dir,
        database,
        connection_timeout,
        max_connections,
        xfer_cleanup_days,
        back_retention_days,
        tombstone_retention_days,
        log_retention_days,
        peers,
    })
}

fn parse_url(raw: &str, config_dir: &Path, inside_kitchensync: bool) -> Result<PeerUrl, String> {
    if raw.starts_with("file://") {
        let path_part = &raw[7..];
        let resolved = if path_part.starts_with("./") || path_part.starts_with("../") {
            let mut base = config_dir.to_path_buf();
            if inside_kitchensync {
                // Back up to parent of .kitchensync/
                base = base.parent().unwrap_or(&base).to_path_buf();
            }
            base.join(path_part).to_string_lossy().to_string()
        } else {
            path_part.to_string()
        };
        Ok(PeerUrl {
            scheme: UrlScheme::File,
            user: None,
            password: None,
            host: None,
            port: 0,
            path: resolved,
            raw: raw.to_string(),
        })
    } else if raw.starts_with("sftp://") {
        let rest = &raw[7..];
        // Parse user[:password]@host[:port]/path
        let (userinfo, hostpath) = if let Some(at_pos) = rest.find('@') {
            (&rest[..at_pos], &rest[at_pos + 1..])
        } else {
            return Err(format!("SFTP URL missing user@: {}", raw));
        };

        let (user, password) = if let Some(colon) = userinfo.find(':') {
            let u = &userinfo[..colon];
            let p = percent_decode(&userinfo[colon + 1..]);
            (u.to_string(), Some(p))
        } else {
            (userinfo.to_string(), None)
        };

        let (hostport, path) = if let Some(slash) = hostpath.find('/') {
            (&hostpath[..slash], hostpath[slash..].to_string())
        } else {
            return Err(format!("SFTP URL missing path: {}", raw));
        };

        let (host, port) = if let Some(colon) = hostport.rfind(':') {
            let h = &hostport[..colon];
            let p: u16 = hostport[colon + 1..]
                .parse()
                .map_err(|_| format!("Invalid port in URL: {}", raw))?;
            (h.to_string(), p)
        } else {
            (hostport.to_string(), 22)
        };

        Ok(PeerUrl {
            scheme: UrlScheme::Sftp,
            user: Some(user),
            password,
            host: Some(host),
            port,
            path,
            raw: raw.to_string(),
        })
    } else {
        Err(format!("Unsupported URL scheme: {}", raw))
    }
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .to_string()
}

/// Simple regex matching without pulling in the regex crate.
struct SimpleRegex {
    pattern: String,
}

impl SimpleRegex {
    fn is_match(&self, s: &str) -> bool {
        // [a-zA-Z0-9][a-zA-Z0-9_-]*
        let chars: Vec<char> = s.chars().collect();
        if chars.is_empty() {
            return false;
        }
        if !chars[0].is_ascii_alphanumeric() {
            return false;
        }
        for &c in &chars[1..] {
            if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
                return false;
            }
        }
        true
    }
}

fn regex_lite(_pattern: &str) -> SimpleRegex {
    SimpleRegex {
        pattern: _pattern.to_string(),
    }
}

pub fn resolve_database_path(config: &Config) -> PathBuf {
    let db = &config.database;
    let p = Path::new(db);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        config.config_dir.join(db)
    }
}
