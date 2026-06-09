use std::sync::Arc;
use crate::api::*;

struct UrlNormalizeImpl;

impl UrlNormalize for UrlNormalizeImpl {
    fn normalize(&self, url: &str) -> String {
        normalize(url)
    }
}

pub fn new() -> Arc<dyn UrlNormalize> {
    Arc::new(UrlNormalizeImpl)
}

fn normalize(url: &str) -> String {
    match find_scheme_end(url) {
        None => normalize_bare_path(url),
        Some(pos) => {
            let scheme = url[..pos].to_lowercase();
            let rest = &url[pos + 3..];
            match scheme.as_str() {
                "file" => normalize_file_url(rest),
                "sftp" => normalize_sftp_url(rest),
                _ => normalize_generic_url(&scheme, rest),
            }
        }
    }
}

// Returns the byte index of the ':' in "://" if the URL has a valid scheme.
fn find_scheme_end(url: &str) -> Option<usize> {
    let pos = url.find("://")?;
    let scheme = &url[..pos];
    if scheme.is_empty() {
        return None;
    }
    let mut chars = scheme.chars();
    if !chars.next()?.is_ascii_alphabetic() {
        return None;
    }
    if chars.any(|c| !c.is_ascii_alphanumeric() && c != '+' && c != '-' && c != '.') {
        return None;
    }
    Some(pos)
}

// True for paths like "c:/..." or "C:\..." (Windows drive letter).
fn is_windows_drive_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b[2] == b'/' || b[2] == b'\\')
}

fn normalize_bare_path(url: &str) -> String {
    let path = strip_query(url);

    if is_windows_drive_path(path) {
        // "c:/photos/" -> file URL with path "/c:/photos"
        let normalized = normalize_url_path(&path.replace('\\', "/"));
        let file_path = format!("/{}", normalized);
        format!("file://{}", percent_decode_unreserved(&file_path))
    } else {
        let p = std::path::Path::new(path);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("/"))
                .join(p)
        };
        let s = normalize_unix_path(&abs.to_string_lossy());
        format!("file://{}", percent_decode_unreserved(&s))
    }
}

fn normalize_file_url(rest: &str) -> String {
    // rest is everything after "file://"; authority (usually empty) then path.
    let path_raw = if rest.starts_with('/') {
        rest
    } else {
        match rest.find('/') {
            Some(idx) => &rest[idx..],
            None => "/",
        }
    };

    let path = strip_query(path_raw);
    let normalized = normalize_url_path(path);

    let abs = if normalized.starts_with('/') {
        normalized
    } else {
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("/"));
        normalize_unix_path(&cwd.join(&normalized).to_string_lossy())
    };

    format!("file://{}", percent_decode_unreserved(&abs))
}

fn normalize_sftp_url(rest: &str) -> String {
    let (authority, path_and_query) = split_authority_path(rest);
    let (user, host_port) = split_user_host(authority);
    let (host, port) = split_host_port(host_port);

    let host_lower = host.to_lowercase();

    let os_user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let effective_user = user
        .map(|u| u.to_string())
        .or_else(|| if os_user.is_empty() { None } else { Some(os_user) });

    // Port 22 is the SFTP default; omit it from the canonical form.
    let keep_port = port.filter(|&p| p != 22);

    let path = strip_query(path_and_query);
    let decoded_path = percent_decode_unreserved(&normalize_url_path(path));

    let mut result = String::from("sftp://");
    if let Some(u) = effective_user {
        result.push_str(&percent_decode_unreserved(&u));
        result.push('@');
    }
    result.push_str(&percent_decode_unreserved(&host_lower));
    if let Some(p) = keep_port {
        result.push(':');
        result.push_str(&p.to_string());
    }
    result.push_str(&decoded_path);
    result
}

fn normalize_generic_url(scheme: &str, rest: &str) -> String {
    let (authority, path_and_query) = split_authority_path(rest);
    let (user, host_port) = split_user_host(authority);
    let (host, port) = split_host_port(host_port);

    let host_lower = host.to_lowercase();
    let decoded_path =
        percent_decode_unreserved(&normalize_url_path(strip_query(path_and_query)));

    let mut result = format!("{}://", scheme);
    if let Some(u) = user {
        result.push_str(&percent_decode_unreserved(u));
        result.push('@');
    }
    result.push_str(&percent_decode_unreserved(&host_lower));
    if let Some(p) = port {
        result.push(':');
        result.push_str(&p.to_string());
    }
    result.push_str(&decoded_path);
    result
}

fn split_authority_path(rest: &str) -> (&str, &str) {
    match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    }
}

fn split_user_host(authority: &str) -> (Option<&str>, &str) {
    match authority.find('@') {
        Some(idx) => (Some(&authority[..idx]), &authority[idx + 1..]),
        None => (None, authority),
    }
}

fn split_host_port(host_port: &str) -> (&str, Option<u16>) {
    if host_port.starts_with('[') {
        if let Some(close) = host_port.find(']') {
            let host = &host_port[..=close];
            let after = &host_port[close + 1..];
            if let Some(tail) = after.strip_prefix(':') {
                if let Ok(p) = tail.parse::<u16>() {
                    return (host, Some(p));
                }
            }
            return (host, None);
        }
    }
    match host_port.rfind(':') {
        Some(idx) => {
            let tail = &host_port[idx + 1..];
            if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
                (&host_port[..idx], tail.parse::<u16>().ok())
            } else {
                (host_port, None)
            }
        }
        None => (host_port, None),
    }
}

fn strip_query(s: &str) -> &str {
    match s.find('?') {
        Some(idx) => &s[..idx],
        None => s,
    }
}

// Collapse consecutive slashes and remove a trailing slash; root "/" is kept.
fn normalize_url_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }

    let mut result = String::with_capacity(path.len());
    let mut last_slash = false;
    for c in path.chars() {
        if c == '/' {
            if !last_slash {
                result.push('/');
            }
            last_slash = true;
        } else {
            result.push(c);
            last_slash = false;
        }
    }

    if result.len() > 1 && result.ends_with('/') {
        result.pop();
    }
    result
}

// Normalize dot-segments in a Unix-style absolute or relative path string.
fn normalize_unix_path(path: &str) -> String {
    let is_absolute = path.starts_with('/');
    let mut components: Vec<&str> = Vec::new();

    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            c => components.push(c),
        }
    }

    let joined = components.join("/");
    if is_absolute {
        format!("/{}", joined)
    } else {
        joined
    }
}

// Decode only unreserved characters (RFC 3986) from percent-encoded sequences.
fn percent_decode_unreserved(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = bytes[i + 1];
            let lo = bytes[i + 2];
            if hi.is_ascii_hexdigit() && lo.is_ascii_hexdigit() {
                let val = hex_val(hi) * 16 + hex_val(lo);
                if is_unreserved(val) {
                    result.push(val as char);
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

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

fn is_unreserved(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~')
}
