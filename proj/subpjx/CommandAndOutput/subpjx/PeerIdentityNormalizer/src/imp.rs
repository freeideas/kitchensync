use crate::api::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use url::Url;

struct PeerIdentityNormalizerImpl;

impl PeerIdentityNormalizer for PeerIdentityNormalizerImpl {
    fn normalize_peer_identity(
        &self,
        target: PeerIdentityTarget,
        current_working_directory: PathBuf,
        current_os_username: String,
    ) -> Result<String, PeerIdentityNormalizationError> {
        match target.clone() {
            PeerIdentityTarget::Local(local) => {
                normalize_local(local, current_working_directory).map_err(|message| {
                    PeerIdentityNormalizationError { target, message }
                })
            }
            PeerIdentityTarget::Sftp(sftp) => Ok(normalize_sftp(sftp, current_os_username)),
        }
    }
}

fn normalize_local(
    target: LocalPeerIdentityTarget,
    current_working_directory: PathBuf,
) -> Result<String, String> {
    if let Ok(url) = Url::parse(&target.path_or_url) {
        if url.scheme().eq_ignore_ascii_case("file") {
            return Ok(normalize_file_url(url));
        }
    }

    if let Some(path) = windows_drive_path(&target.path_or_url) {
        return Ok(format!("file:///{}", normalize_path(&path)));
    }

    let input_path = Path::new(&target.path_or_url);
    let absolute_path = if input_path.is_absolute() {
        input_path.to_path_buf()
    } else {
        current_working_directory.join(input_path)
    };

    Url::from_file_path(&absolute_path)
        .map(normalize_file_url)
        .map_err(|_| "local path cannot be represented as a file URL".to_string())
}

fn normalize_file_url(mut url: Url) -> String {
    url.set_query(None);
    let scheme = url.scheme().to_ascii_lowercase();
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let path = normalize_path(url.path());

    if host.is_empty() {
        format!("{scheme}:///{path}")
    } else {
        format!("{scheme}://{host}/{path}")
    }
}

fn normalize_sftp(target: SftpPeerIdentityTarget, current_os_username: String) -> String {
    let host = target.host.to_ascii_lowercase();
    let username = target.username.unwrap_or(current_os_username);
    let path = normalize_path(&target.absolute_path);
    let port = if target.port == 22 {
        String::new()
    } else {
        format!(":{}", target.port)
    };

    format!("sftp://{username}@{host}{port}/{path}")
}

fn windows_drive_path(path: &str) -> Option<String> {
    let mut chars = path.chars();
    let drive = chars.next()?;
    if !drive.is_ascii_alphabetic() || chars.next()? != ':' {
        return None;
    }

    let rest: String = chars.collect();
    if !rest.starts_with('\\') && !rest.starts_with('/') {
        return None;
    }

    Some(format!(
        "{}:{}",
        drive.to_ascii_uppercase(),
        rest.replace('\\', "/")
    ))
}

fn normalize_path(path: &str) -> String {
    let mut collapsed = String::new();
    let mut previous_slash = false;

    for ch in path.chars() {
        if ch == '/' {
            if !previous_slash {
                collapsed.push(ch);
            }
            previous_slash = true;
        } else {
            collapsed.push(ch);
            previous_slash = false;
        }
    }

    while collapsed.len() > 1 && collapsed.ends_with('/') {
        collapsed.pop();
    }

    decode_unreserved_percent_encoding(collapsed.trim_start_matches('/'))
}

fn decode_unreserved_percent_encoding(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut output = String::with_capacity(path.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Some(decoded) = decode_hex_pair(bytes[index + 1], bytes[index + 2]) {
                if is_unreserved(decoded) {
                    output.push(decoded as char);
                    index += 3;
                    continue;
                }
            }
        }

        let ch = path[index..]
            .chars()
            .next()
            .expect("index always starts at a character boundary");
        output.push(ch);
        index += ch.len_utf8();
    }

    output
}

fn decode_hex_pair(high: u8, low: u8) -> Option<u8> {
    Some(hex_value(high)? * 16 + hex_value(low)?)
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
    value.is_ascii_alphanumeric() || matches!(value, b'-' | b'.' | b'_' | b'~')
}

pub fn new() -> std::sync::Arc<dyn PeerIdentityNormalizer> {
    Arc::new(PeerIdentityNormalizerImpl)
}
