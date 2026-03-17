use std::path::Path;
use std::fs;

#[derive(Debug, Clone)]
pub struct GlobalConfig {
    pub queue_max_size: usize,
    pub connection_timeout: u32,
    pub retry_interval: u32,
    pub workers_per_peer: usize,
    pub xfer_cleanup_days: u32,
    pub back_retention_days: u32,
    pub tombstone_retention_days: u32,
    pub log_retention_days: u32,
    pub peers: Vec<PeerConfig>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            queue_max_size: 10000,
            connection_timeout: 30,
            retry_interval: 60,
            workers_per_peer: 10,
            xfer_cleanup_days: 2,
            back_retention_days: 90,
            tombstone_retention_days: 180,
            log_retention_days: 32,
            peers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PeerConfig {
    pub name: String,
    pub urls: Vec<String>,
    pub rewalk_after_minutes: u32,
}

impl Default for PeerConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            urls: Vec::new(),
            rewalk_after_minutes: 720,
        }
    }
}

pub fn parse_peers_conf(path: &Path) -> Result<GlobalConfig, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read peers.conf: {}", e))?;
    parse_peers_conf_content(&content)
}

pub fn parse_peers_conf_content(content: &str) -> Result<GlobalConfig, String> {
    let mut config = GlobalConfig::default();
    let mut current_peer: Option<PeerConfig> = None;

    let global_settings = [
        "queue-max-size",
        "connection-timeout",
        "retry-interval",
        "workers-per-peer",
        "xfer-cleanup-days",
        "back-retention-days",
        "tombstone-retention-days",
        "log-retention-days",
    ];

    let peer_settings = ["rewalk-after-minutes"];

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Check if line is indented (peer URL or per-peer setting)
        let is_indented = content.lines()
            .find(|l| l.trim() == line)
            .map(|l| l.starts_with(' ') || l.starts_with('\t'))
            .unwrap_or(false);

        if is_indented {
            // This is either a per-peer setting or a URL
            let peer = current_peer.as_mut().ok_or("URL or setting found before peer name")?;

            // Check if it's a per-peer setting
            let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
            if parts.len() == 2 && peer_settings.contains(&parts[0]) {
                match parts[0] {
                    "rewalk-after-minutes" => {
                        peer.rewalk_after_minutes = parts[1].trim().parse()
                            .map_err(|_| format!("Invalid value for rewalk-after-minutes: {}", parts[1]))?;
                    }
                    _ => {}
                }
            } else {
                // It's a URL
                peer.urls.push(line.to_string());
            }
        } else {
            // Not indented - either a global setting or a peer name

            // Check if it's a global setting
            let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
            if parts.len() == 2 && global_settings.contains(&parts[0]) {
                let value = parts[1].trim();
                match parts[0] {
                    "queue-max-size" => {
                        config.queue_max_size = value.parse()
                            .map_err(|_| format!("Invalid value for queue-max-size: {}", value))?;
                    }
                    "connection-timeout" => {
                        config.connection_timeout = value.parse()
                            .map_err(|_| format!("Invalid value for connection-timeout: {}", value))?;
                    }
                    "retry-interval" => {
                        config.retry_interval = value.parse()
                            .map_err(|_| format!("Invalid value for retry-interval: {}", value))?;
                    }
                    "workers-per-peer" => {
                        config.workers_per_peer = value.parse()
                            .map_err(|_| format!("Invalid value for workers-per-peer: {}", value))?;
                    }
                    "xfer-cleanup-days" => {
                        config.xfer_cleanup_days = value.parse()
                            .map_err(|_| format!("Invalid value for xfer-cleanup-days: {}", value))?;
                    }
                    "back-retention-days" => {
                        config.back_retention_days = value.parse()
                            .map_err(|_| format!("Invalid value for back-retention-days: {}", value))?;
                    }
                    "tombstone-retention-days" => {
                        config.tombstone_retention_days = value.parse()
                            .map_err(|_| format!("Invalid value for tombstone-retention-days: {}", value))?;
                    }
                    "log-retention-days" => {
                        config.log_retention_days = value.parse()
                            .map_err(|_| format!("Invalid value for log-retention-days: {}", value))?;
                    }
                    _ => {}
                }
            } else {
                // It's a peer name
                // Save previous peer if exists
                if let Some(peer) = current_peer.take() {
                    if !peer.urls.is_empty() {
                        config.peers.push(peer);
                    }
                }

                // Validate peer name
                let name = parts[0];
                if !is_valid_peer_name(name) {
                    return Err(format!("Invalid peer name: {}. Must match [a-zA-Z0-9][a-zA-Z0-9_-]* and be <= 64 characters", name));
                }

                current_peer = Some(PeerConfig {
                    name: name.to_string(),
                    ..Default::default()
                });
            }
        }
    }

    // Save last peer
    if let Some(peer) = current_peer {
        if !peer.urls.is_empty() {
            config.peers.push(peer);
        }
    }

    Ok(config)
}

fn is_valid_peer_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }

    let chars: Vec<char> = name.chars().collect();

    // First character must be alphanumeric
    if !chars[0].is_ascii_alphanumeric() {
        return false;
    }

    // Rest must be alphanumeric, underscore, or hyphen
    chars[1..].iter().all(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
}
