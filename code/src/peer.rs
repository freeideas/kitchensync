use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PeerRole {
    Canon,
    Subordinate,
    Bidirectional,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scheme {
    Local,
    Sftp,
}

#[derive(Debug, Clone)]
pub struct PeerUrl {
    pub scheme: Scheme,
    pub host: Option<String>,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub path: String,
    pub max_connections: Option<usize>,
    pub connect_timeout: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct PeerSpec {
    pub role: PeerRole,
    pub urls: Vec<PeerUrl>,
}

impl fmt::Display for PeerSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.role {
            PeerRole::Canon => "+",
            PeerRole::Subordinate => "-",
            PeerRole::Bidirectional => "",
        };
        if self.urls.len() == 1 {
            write!(f, "{}{}", prefix, self.urls[0].path)
        } else {
            write!(f, "{}[{} urls]", prefix, self.urls.len())
        }
    }
}

/// Parse a single CLI peer argument into a PeerSpec.
/// Handles +/- prefixes, bracket fallback syntax, and sftp:// URLs with query params.
pub fn parse_peer(arg: &str) -> Result<PeerSpec, String> {
    let (role, rest) = parse_prefix(arg);

    let url_strings = if rest.starts_with('[') && rest.ends_with(']') {
        split_bracket_urls(&rest[1..rest.len() - 1])
    } else {
        vec![rest.to_string()]
    };

    let urls: Result<Vec<PeerUrl>, String> = url_strings.iter().map(|s| parse_url(s)).collect();

    Ok(PeerSpec {
        role,
        urls: urls?,
    })
}

fn parse_prefix(arg: &str) -> (PeerRole, &str) {
    if let Some(rest) = arg.strip_prefix('+') {
        (PeerRole::Canon, rest)
    } else if let Some(rest) = arg.strip_prefix('-') {
        // Avoid treating --flags as subordinate peers
        if rest.starts_with('-') {
            (PeerRole::Bidirectional, arg)
        } else {
            (PeerRole::Subordinate, rest)
        }
    } else {
        (PeerRole::Bidirectional, arg)
    }
}

/// Split comma-separated URLs inside brackets, respecting nested structure.
fn split_bracket_urls(s: &str) -> Vec<String> {
    s.split(',').map(|part| part.trim().to_string()).collect()
}

fn parse_url(s: &str) -> Result<PeerUrl, String> {
    let lower = s.to_lowercase();
    let mut url = if lower.starts_with("sftp://") {
        parse_sftp_url(s)?
    } else if lower.starts_with("file://") {
        let raw_path = &s[7..];
        // REQ_DB_022: Strip query parameters
        let path = raw_path.split('?').next().unwrap_or(raw_path).to_string();
        PeerUrl {
            scheme: Scheme::Local,
            host: None,
            port: 22,
            username: None,
            password: None,
            path,
            max_connections: None,
            connect_timeout: None,
        }
    } else {
        // Bare path → file:// (REQ_DB_019)
        // REQ_DB_022: Strip query parameters
        let path = s.split('?').next().unwrap_or(s).to_string();
        PeerUrl {
            scheme: Scheme::Local,
            host: None,
            port: 22,
            username: None,
            password: None,
            path,
            max_connections: None,
            connect_timeout: None,
        }
    };
    normalize_url(&mut url);
    Ok(url)
}

/// Normalize a PeerUrl per REQ_DB_015 through REQ_DB_022.
fn normalize_url(url: &mut PeerUrl) {
    // REQ_DB_015: Lowercase hostname
    if let Some(ref mut host) = url.host {
        *host = host.to_lowercase();
    }

    // REQ_DB_016: Remove default port (22 for SFTP)
    // (port stays as 22 internally but won't affect identity)

    // REQ_DB_021: Percent-decode unreserved characters
    url.path = percent_decode_unreserved(&url.path);

    // REQ_DB_017: Collapse consecutive slashes in path
    while url.path.contains("//") {
        url.path = url.path.replace("//", "/");
    }

    // REQ_DB_018: Remove trailing slash
    while url.path.len() > 1 && url.path.ends_with('/') {
        url.path.pop();
    }

    // REQ_DB_020: file:// URLs resolve to absolute path from cwd
    if url.scheme == Scheme::Local {
        if let Ok(abs) = std::fs::canonicalize(&url.path) {
            url.path = abs.to_string_lossy().to_string();
            // Ensure forward slashes on all platforms
            url.path = url.path.replace('\\', "/");
        }
    }
}

/// Percent-decode unreserved characters (REQ_DB_021).
fn percent_decode_unreserved(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                if byte.is_ascii_alphanumeric()
                    || byte == b'-'
                    || byte == b'.'
                    || byte == b'_'
                    || byte == b'~'
                {
                    result.push(byte as char);
                    i += 3;
                    continue;
                }
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn parse_sftp_url(s: &str) -> Result<PeerUrl, String> {
    // sftp://[user[:password]@]host[:port]/path[?params]
    // Handle case-insensitive scheme (SFTP://, Sftp://, etc.)
    let scheme_end = s.find("://").unwrap_or(4) + 3;
    let after_scheme = &s[scheme_end..]; // skip "sftp://" (any case)

    // Split off query string
    let (main_part, query) = match after_scheme.find('?') {
        Some(i) => (&after_scheme[..i], Some(&after_scheme[i + 1..])),
        None => (after_scheme, None),
    };

    // Split user info from host+path
    let (user_info, host_path) = match main_part.find('@') {
        Some(i) => (Some(&main_part[..i]), &main_part[i + 1..]),
        None => (None, main_part),
    };

    let (username, password) = match user_info {
        Some(info) => match info.find(':') {
            Some(i) => (Some(info[..i].to_string()), Some(info[i + 1..].to_string())),
            None => (Some(info.to_string()), None),
        },
        None => (None, None),
    };

    // Split host[:port] from /path
    let (host_port, path) = match host_path.find('/') {
        Some(i) => (&host_path[..i], host_path[i..].to_string()),
        None => (host_path, "/".to_string()),
    };

    let (host, port) = match host_port.rfind(':') {
        Some(i) => {
            let maybe_port = &host_port[i + 1..];
            match maybe_port.parse::<u16>() {
                Ok(p) => (host_port[..i].to_string(), p),
                Err(_) => (host_port.to_string(), 22),
            }
        }
        None => (host_port.to_string(), 22),
    };

    // Parse query params
    let (mc, ct) = parse_query_params(query);

    Ok(PeerUrl {
        scheme: Scheme::Sftp,
        host: Some(host),
        port,
        username,
        password,
        path,
        max_connections: mc,
        connect_timeout: ct,
    })
}

fn parse_query_params(query: Option<&str>) -> (Option<usize>, Option<u64>) {
    let mut mc = None;
    let mut ct = None;

    if let Some(q) = query {
        for param in q.split('&') {
            if let Some((key, val)) = param.split_once('=') {
                match key {
                    "mc" => mc = val.parse().ok(),
                    "ct" => ct = val.parse().ok(),
                    _ => {}
                }
            }
        }
    }

    (mc, ct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_local_path() {
        let spec = parse_peer("c:/photos").unwrap();
        assert_eq!(spec.role, PeerRole::Bidirectional);
        assert_eq!(spec.urls.len(), 1);
        assert_eq!(spec.urls[0].scheme, Scheme::Local);
        assert_eq!(spec.urls[0].path, "c:/photos");
    }

    #[test]
    fn test_parse_canon_prefix() {
        let spec = parse_peer("+c:/photos").unwrap();
        assert_eq!(spec.role, PeerRole::Canon);
        assert_eq!(spec.urls[0].path, "c:/photos");
    }

    #[test]
    fn test_parse_subordinate_prefix() {
        let spec = parse_peer("-/mnt/usb/photos").unwrap();
        assert_eq!(spec.role, PeerRole::Subordinate);
        assert_eq!(spec.urls[0].path, "/mnt/usb/photos");
    }

    #[test]
    fn test_parse_sftp_url() {
        let spec = parse_peer("sftp://bilbo@cloud/volume1/photos").unwrap();
        assert_eq!(spec.role, PeerRole::Bidirectional);
        assert_eq!(spec.urls[0].scheme, Scheme::Sftp);
        assert_eq!(spec.urls[0].host.as_deref(), Some("cloud"));
        assert_eq!(spec.urls[0].username.as_deref(), Some("bilbo"));
        assert_eq!(spec.urls[0].path, "/volume1/photos");
    }

    #[test]
    fn test_parse_bracket_fallbacks() {
        let spec =
            parse_peer("[h:/office-share/photos,sftp://192.168.1.50:2222/photos,sftp://cloud.vpn/photos]")
                .unwrap();
        assert_eq!(spec.urls.len(), 3);
        assert_eq!(spec.urls[0].scheme, Scheme::Local);
        assert_eq!(spec.urls[1].port, 2222);
        assert_eq!(spec.urls[2].host.as_deref(), Some("cloud.vpn"));
    }

    #[test]
    fn test_parse_query_params() {
        let spec = parse_peer("sftp://host/path?mc=20&ct=60").unwrap();
        assert_eq!(spec.urls[0].max_connections, Some(20));
        assert_eq!(spec.urls[0].connect_timeout, Some(60));
    }
}
