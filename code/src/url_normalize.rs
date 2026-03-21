use percent_encoding::percent_decode_str;
use std::path::Path;

/// Normalize a URL for storage and lookup.
pub fn normalize_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();

    // Detect scheme
    if let Some(rest) = raw.strip_prefix("sftp://").or_else(|| raw.strip_prefix("SFTP://")) {
        normalize_sftp(rest)
    } else if let Some(rest) = raw.strip_prefix("file://") {
        normalize_file_url(rest)
    } else {
        // Bare path → file://
        normalize_bare_path(raw)
    }
}

fn normalize_sftp(rest: &str) -> Result<String, String> {
    // rest = [user[:password]@]host[:port]/path
    let (userinfo, hostpath) = if let Some(at_pos) = find_last_at(rest) {
        (&rest[..at_pos], &rest[at_pos + 1..])
    } else {
        ("", rest)
    };

    let (host_port, path) = if let Some(slash_pos) = hostpath.find('/') {
        (&hostpath[..slash_pos], &hostpath[slash_pos..])
    } else {
        return Err("SFTP URL must have a path".to_string());
    };

    // Separate host and port
    let (host, port) = if let Some(colon_pos) = host_port.rfind(':') {
        let potential_port = &host_port[colon_pos + 1..];
        if let Ok(p) = potential_port.parse::<u16>() {
            (&host_port[..colon_pos], Some(p))
        } else {
            (host_port, None)
        }
    } else {
        (host_port, None)
    };

    let host_lower = host.to_lowercase();

    // Normalize path: collapse consecutive slashes, remove trailing slash
    let path_normalized = normalize_path(path);

    // Percent-decode unreserved characters
    let path_decoded = decode_unreserved(&path_normalized);

    let mut result = String::from("sftp://");
    if !userinfo.is_empty() {
        result.push_str(userinfo);
        result.push('@');
    }
    result.push_str(&host_lower);

    // Remove default port 22
    if let Some(p) = port {
        if p != 22 {
            result.push(':');
            result.push_str(&p.to_string());
        }
    }
    result.push_str(&path_decoded);

    Ok(result)
}

fn normalize_file_url(rest: &str) -> Result<String, String> {
    let path = normalize_path(&format!("/{}", rest.trim_start_matches('/')));
    let decoded = decode_unreserved(&path);
    Ok(format!("file://{}", decoded))
}

fn normalize_bare_path(raw: &str) -> Result<String, String> {
    let path = if raw.starts_with('/') || (raw.len() >= 2 && raw.as_bytes()[1] == b':') {
        // Absolute path (Unix or Windows drive letter)
        raw.to_string()
    } else if raw.starts_with("./") || raw.starts_with("../") || raw == "." || raw == ".." {
        // Relative path - resolve from cwd
        let cwd = std::env::current_dir()
            .map_err(|e| format!("cannot get cwd: {}", e))?;
        let resolved = cwd.join(raw);
        let resolved = normalize_path_components(&resolved);
        resolved
    } else {
        // Treat as relative
        let cwd = std::env::current_dir()
            .map_err(|e| format!("cannot get cwd: {}", e))?;
        let resolved = cwd.join(raw);
        let resolved = normalize_path_components(&resolved);
        resolved
    };

    let path = path.replace('\\', "/");
    let normalized = normalize_path(&path);
    Ok(format!("file://{}", normalized))
}

fn normalize_path(path: &str) -> String {
    // Collapse consecutive slashes
    let mut result = String::with_capacity(path.len());
    let mut last_was_slash = false;
    for ch in path.chars() {
        if ch == '/' {
            if !last_was_slash {
                result.push('/');
            }
            last_was_slash = true;
        } else {
            result.push(ch);
            last_was_slash = false;
        }
    }
    // Remove trailing slash (but keep root /)
    if result.len() > 1 && result.ends_with('/') {
        result.pop();
    }
    result
}

fn normalize_path_components(path: &Path) -> String {
    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            std::path::Component::RootDir => components.push(String::from("/")),
            std::path::Component::Normal(s) => components.push(s.to_string_lossy().to_string()),
            std::path::Component::ParentDir => { components.pop(); },
            std::path::Component::CurDir => {},
            std::path::Component::Prefix(p) => components.push(p.as_os_str().to_string_lossy().to_string()),
        }
    }
    if components.first().map(|s| s.as_str()) == Some("/") {
        format!("/{}", components[1..].join("/"))
    } else {
        components.join("/")
    }
}

fn decode_unreserved(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().to_string()
}

fn find_last_at(s: &str) -> Option<usize> {
    // Find the '@' that separates userinfo from host.
    // We want the last '@' before the first '/' (path start).
    let path_start = s.find('/').unwrap_or(s.len());
    s[..path_start].rfind('@')
}

/// Strip trailing '!' from a URL, returning (url, is_canon).
pub fn strip_canon_marker(url: &str) -> (&str, bool) {
    if url.ends_with('!') {
        (&url[..url.len() - 1], true)
    } else {
        (url, false)
    }
}
