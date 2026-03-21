use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::url_normalize::{normalize_url, strip_canon_marker};

/// Top-level config file structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default, rename = "max-connections")]
    pub max_connections: Option<u32>,
    #[serde(default, rename = "connection-timeout")]
    pub connection_timeout: Option<u32>,
    #[serde(default, rename = "xfer-cleanup-days")]
    pub xfer_cleanup_days: Option<u32>,
    #[serde(default, rename = "back-retention-days")]
    pub back_retention_days: Option<u32>,
    #[serde(default, rename = "tombstone-retention-days")]
    pub tombstone_retention_days: Option<u32>,
    #[serde(default, rename = "log-retention-days")]
    pub log_retention_days: Option<u32>,
    #[serde(default, rename = "log-level")]
    pub log_level: Option<String>,
    #[serde(default)]
    pub peer_groups: Vec<PeerGroup>,
}

impl Config {
    pub fn max_connections(&self) -> u32 {
        self.max_connections.unwrap_or(10)
    }
    pub fn connection_timeout(&self) -> u32 {
        self.connection_timeout.unwrap_or(30)
    }
    pub fn xfer_cleanup_days(&self) -> u32 {
        self.xfer_cleanup_days.unwrap_or(2)
    }
    pub fn back_retention_days(&self) -> u32 {
        self.back_retention_days.unwrap_or(90)
    }
    pub fn tombstone_retention_days(&self) -> u32 {
        self.tombstone_retention_days.unwrap_or(180)
    }
    pub fn log_retention_days(&self) -> u32 {
        self.log_retention_days.unwrap_or(32)
    }
    pub fn log_level(&self) -> &str {
        self.log_level.as_deref().unwrap_or("info")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerGroup {
    pub name: String,
    pub peers: Vec<PeerEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerEntry {
    pub name: String,
    pub urls: Vec<UrlEntry>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub canon: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UrlEntry {
    Simple(String),
    WithSettings(UrlWithSettings),
}

impl UrlEntry {
    pub fn url_str(&self) -> &str {
        match self {
            UrlEntry::Simple(s) => s,
            UrlEntry::WithSettings(u) => &u.url,
        }
    }

    pub fn max_connections(&self) -> Option<u32> {
        match self {
            UrlEntry::Simple(_) => None,
            UrlEntry::WithSettings(u) => u.max_connections,
        }
    }

    pub fn connection_timeout(&self) -> Option<u32> {
        match self {
            UrlEntry::Simple(_) => None,
            UrlEntry::WithSettings(u) => u.connection_timeout,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlWithSettings {
    pub url: String,
    #[serde(default, rename = "max-connections", skip_serializing_if = "Option::is_none")]
    pub max_connections: Option<u32>,
    #[serde(default, rename = "connection-timeout", skip_serializing_if = "Option::is_none")]
    pub connection_timeout: Option<u32>,
}

/// Strip // and /* */ comments from JSON text.
pub fn strip_json_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_string = false;

    while i < len {
        if in_string {
            out.push(chars[i]);
            if chars[i] == '\\' && i + 1 < len {
                i += 1;
                out.push(chars[i]);
            } else if chars[i] == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if chars[i] == '"' {
            in_string = true;
            out.push(chars[i]);
            i += 1;
            continue;
        }

        if chars[i] == '/' && i + 1 < len {
            if chars[i + 1] == '/' {
                // Line comment
                i += 2;
                while i < len && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            } else if chars[i + 1] == '*' {
                // Block comment
                i += 2;
                while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                if i + 1 < len {
                    i += 2;
                }
                continue;
            }
        }

        out.push(chars[i]);
        i += 1;
    }
    out
}

pub fn load_config(path: &Path) -> Result<Config, String> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read config: {}", e))?;
    let stripped = strip_json_comments(&text);
    serde_json::from_str(&stripped)
        .map_err(|e| format!("invalid config JSON: {}", e))
}

pub fn save_config(path: &Path, config: &Config) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create config directory: {}", e))?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("cannot serialize config: {}", e))?;
    std::fs::write(path, json)
        .map_err(|e| format!("cannot write config: {}", e))
}

/// Resolve the config directory path from --cfg argument.
pub fn resolve_config_dir(cfg_arg: Option<&str>) -> PathBuf {
    match cfg_arg {
        None => {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".kitchensync")
        }
        Some(path) => {
            let path = path.trim();
            if path.is_empty() {
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| ".".to_string());
                return PathBuf::from(home).join(".kitchensync");
            }
            if path.ends_with(".kitchensync/") || path.ends_with(".kitchensync") {
                PathBuf::from(if path.ends_with('/') { path.to_string() } else { format!("{}/", path) }.trim_end_matches('/'))
            } else {
                PathBuf::from(path).join(".kitchensync")
            }
        }
    }
}

/// Parsed CLI arguments.
pub struct CliArgs {
    pub cfg_path: Option<String>,
    pub urls: Vec<(String, bool)>, // (url, is_canon)
    pub settings: HashMap<String, String>,
    pub help: bool,
}

pub fn parse_args(args: &[String]) -> CliArgs {
    let mut result = CliArgs {
        cfg_path: None,
        urls: Vec::new(),
        settings: HashMap::new(),
        help: false,
    };

    if args.is_empty() {
        result.help = true;
        return result;
    }

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-h" || arg == "--help" {
            result.help = true;
            return result;
        } else if arg == "--cfgdir" {
            if i + 1 < args.len() && !args[i + 1].starts_with('-') && !args[i + 1].contains('=') {
                i += 1;
                result.cfg_path = Some(args[i].clone());
            } else {
                result.cfg_path = Some(String::new());
            }
        } else if arg.contains('=') {
            let parts: Vec<&str> = arg.splitn(2, '=').collect();
            result.settings.insert(parts[0].to_string(), parts[1].to_string());
        } else {
            let (url, is_canon) = strip_canon_marker(arg);
            result.urls.push((url.to_string(), is_canon));
        }
        i += 1;
    }

    result
}

/// Merge CLI URLs into the config's peer groups.
/// Returns the index of the active group, or creates a new one.
pub fn merge_cli_urls(config: &mut Config, urls: &[(String, bool)]) -> Result<usize, String> {
    if urls.is_empty() {
        return Err("no URLs specified".to_string());
    }

    // Normalize all CLI URLs
    let mut normalized: Vec<(String, bool)> = Vec::new();
    for (url, is_canon) in urls {
        let n = normalize_url(url)?;
        normalized.push((n, *is_canon));
    }

    // Find which group(s) these URLs belong to
    let mut matched_group: Option<usize> = None;
    for (norm_url, _) in &normalized {
        for (gi, group) in config.peer_groups.iter().enumerate() {
            for peer in &group.peers {
                for url_entry in &peer.urls {
                    let entry_norm = normalize_url(url_entry.url_str())
                        .unwrap_or_default();
                    if entry_norm == *norm_url {
                        if let Some(prev) = matched_group {
                            if prev != gi {
                                return Err(format!(
                                    "URLs belong to different groups: '{}' and '{}'",
                                    config.peer_groups[prev].name,
                                    config.peer_groups[gi].name
                                ));
                            }
                        }
                        matched_group = Some(gi);
                    }
                }
            }
        }
    }

    let group_idx = match matched_group {
        Some(gi) => gi,
        None => {
            // Create new group
            let name = format!("group-{}", config.peer_groups.len() + 1);
            config.peer_groups.push(PeerGroup {
                name,
                peers: Vec::new(),
            });
            config.peer_groups.len() - 1
        }
    };

    // Add any new URLs as new peers in the group
    for (norm_url, is_canon) in &normalized {
        let already_exists = config.peer_groups[group_idx].peers.iter().any(|p| {
            p.urls.iter().any(|u| {
                normalize_url(u.url_str()).unwrap_or_default() == *norm_url
            })
        });
        if !already_exists {
            let peer_name = format!("peer-{}", config.peer_groups[group_idx].peers.len() + 1);
            config.peer_groups[group_idx].peers.push(PeerEntry {
                name: peer_name,
                urls: vec![UrlEntry::Simple(norm_url.clone())],
                canon: false,
            });
        }
    }

    Ok(group_idx)
}

/// Merge CLI settings into the config.
pub fn merge_cli_settings(config: &mut Config, settings: &HashMap<String, String>) {
    for (key, value) in settings {
        match key.as_str() {
            "max-connections" => config.max_connections = value.parse().ok(),
            "connection-timeout" => config.connection_timeout = value.parse().ok(),
            "xfer-cleanup-days" => config.xfer_cleanup_days = value.parse().ok(),
            "back-retention-days" => config.back_retention_days = value.parse().ok(),
            "tombstone-retention-days" => config.tombstone_retention_days = value.parse().ok(),
            "log-retention-days" => config.log_retention_days = value.parse().ok(),
            "log-level" => config.log_level = Some(value.clone()),
            _ => {} // ignore unknown settings
        }
    }
}
