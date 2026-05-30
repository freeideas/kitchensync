use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::{PeerRole, PeerSpec, PeerUrl, RelPath, RunConfig, RunRequest, Verbosity};

const HELP: &str = "Usage: kitchensync [options] <peer> <peer> [<peer>...]\n\nSynchronize file trees across multiple peers.\n\nRunning with no arguments prints this help. See the specs for full behavior.\n\nPeers:\n  /path or c:\\path                 Local path (same as file://)\n  sftp://user@host/path            Remote over SSH\n  sftp://user@host:port/path       Non-standard SSH port\n  sftp://host/path                 Remote over SSH, current OS user\n  sftp://user:password@host/path   Inline password (prefer SSH keys)\n\nPrefix modifiers:\n  +<peer>                          Canon - this peer's state wins all conflicts\n  -<peer>                          Subordinate - overwritten to match the group\n\nFallback URLs (multiple paths to the same data):\n  [url1,url2,...]                  Try in order, first that connects wins\n  +[url1,url2,...]                 Canon peer with fallbacks\n  -[url1,url2,...]                 Subordinate peer with fallbacks\n\nPer-URL settings (query string, inside quotes):\n  \"sftp://host/path?timeout-conn=60\"     Connection timeout for this URL\n  \"sftp://host/path?timeout-idle=10\"     SFTP idle keep-alive TTL for this URL\n  \"sftp://host/path?timeout-conn=60&timeout-idle=10\"  Combine multiple\n\nOptions:\n  --dry-run          Read-only and plan, but make no peer changes\n  --max-copies N     Max active file copies across the whole run (default: 10)\n  --retries-copy N   Give up copying after this many tries (default: 3)\n  --retries-list N   Give up listing after this many tries (default: 3)\n  --timeout-conn N   SSH handshake timeout in seconds (default: 30)\n  --timeout-idle N   SFTP idle keep-alive TTL in seconds (default: 30)\n  --verbosity LEVEL  Verbosity: error, info, debug, trace (default: info)\n  -x RELPATH         Exclude relative slash path from sync; repeatable\n  --keep-tmp-days N  Delete stale TMP staging after N days (default: 2)\n  --keep-bak-days N  Delete displaced files (BAK/) after N days (default: 90)\n  --keep-del-days N  Forget deletion records after N days (default: 180)\n\nQuick start:\n  kitchensync +c:/photos sftp://user@host/photos      First sync (c: is canon)\n  kitchensync c:/photos sftp://host/photos            Bidirectional\n  kitchensync c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate\n  kitchensync c:/photos \"sftp://user:p%40ss@host/photos\"  Inline password\n\nCanon (+) is required on first sync when no peer has snapshot history.\nAfter the first sync, bidirectional sync works without canon.\n\nTip: if ssh user@host and cd /path works, sftp://user@host/path will too.\n\nDisplaced files are recoverable from nearby:\n  .kitchensync/BAK/ directories (kept for --keep-bak-days days).\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliParseEnv {
    pub current_dir: PathBuf,
    pub current_user: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliInvocation {
    Help {
        help: &'static str,
    },
    Invalid {
        error: CliArgumentError,
        help: &'static str,
    },
    Run(RunRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgumentError {
    pub message: String,
}

#[derive(Debug, Clone)]
struct Options {
    dry_run: bool,
    max_copies: usize,
    retries_copy: usize,
    retries_list: usize,
    timeout_conn: u32,
    timeout_idle: u32,
    verbosity: Verbosity,
    keep_tmp_days: u32,
    keep_bak_days: u32,
    keep_del_days: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            dry_run: false,
            max_copies: 10,
            retries_copy: 3,
            retries_list: 3,
            timeout_conn: 30,
            timeout_idle: 30,
            verbosity: Verbosity::Info,
            keep_tmp_days: 2,
            keep_bak_days: 90,
            keep_del_days: 180,
        }
    }
}

pub fn summary() -> &'static str {
    "cli: command-line parsing and user-facing output"
}

pub fn help_text() -> &'static str {
    HELP
}

pub fn parse_invocation<I, S>(args: I, env: &CliParseEnv) -> CliInvocation
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| os_string_to_string(arg.into()))
        .collect();

    if args.is_empty() {
        return CliInvocation::Help { help: help_text() };
    }

    match parse_run(args, env) {
        Ok(request) => CliInvocation::Run(request),
        Err(message) => CliInvocation::Invalid {
            error: CliArgumentError { message },
            help: help_text(),
        },
    }
}

fn parse_run(args: Vec<String>, env: &CliParseEnv) -> Result<RunRequest, String> {
    let mut options = Options::default();
    let mut excludes = Vec::new();
    let mut peer_operands = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--dry-run" => {
                options.dry_run = true;
                index += 1;
            }
            "-x" => {
                let value = option_value(&args, index, "-x")?;
                excludes.push(parse_exclude(value)?);
                index += 2;
            }
            "--max-copies" => {
                options.max_copies = parse_next_usize(&args, &mut index, "--max-copies")?;
            }
            "--retries-copy" => {
                options.retries_copy = parse_next_usize(&args, &mut index, "--retries-copy")?;
            }
            "--retries-list" => {
                options.retries_list = parse_next_usize(&args, &mut index, "--retries-list")?;
            }
            "--timeout-conn" => {
                options.timeout_conn = parse_next_u32(&args, &mut index, "--timeout-conn")?;
            }
            "--timeout-idle" => {
                options.timeout_idle = parse_next_u32(&args, &mut index, "--timeout-idle")?;
            }
            "--keep-tmp-days" => {
                options.keep_tmp_days = parse_next_u32(&args, &mut index, "--keep-tmp-days")?;
            }
            "--keep-bak-days" => {
                options.keep_bak_days = parse_next_u32(&args, &mut index, "--keep-bak-days")?;
            }
            "--keep-del-days" => {
                options.keep_del_days = parse_next_u32(&args, &mut index, "--keep-del-days")?;
            }
            "--verbosity" => {
                let value = option_value(&args, index, "--verbosity")?;
                options.verbosity = parse_verbosity(value)?;
                index += 2;
            }
            value if value.starts_with("--") => {
                return Err(format!("unknown option: {value}"));
            }
            value => {
                peer_operands.push(value.to_string());
                index += 1;
            }
        }
    }

    if peer_operands.len() < 2 {
        return Err("too few peer operands: expected at least two".to_string());
    }

    let mut peers = Vec::with_capacity(peer_operands.len());
    let mut canon_count = 0usize;
    for operand in peer_operands {
        let peer = parse_peer_operand(&operand, env)?;
        if peer.role == PeerRole::Canon {
            canon_count += 1;
            if canon_count > 1 {
                return Err("more than one canon peer was provided".to_string());
            }
        }
        peers.push(peer);
    }

    let config_excludes = excludes.clone();

    Ok(RunRequest {
        config: RunConfig {
            dry_run: options.dry_run,
            max_copies: options.max_copies,
            retries_copy: options.retries_copy,
            retries_list: options.retries_list,
            timeout_conn: options.timeout_conn,
            timeout_idle: options.timeout_idle,
            verbosity: options.verbosity,
            keep_tmp_days: options.keep_tmp_days,
            keep_bak_days: options.keep_bak_days,
            keep_del_days: options.keep_del_days,
            excludes: config_excludes,
        },
        peers,
        excludes,
    })
}

fn option_value<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str, String> {
    let value = args
        .get(index + 1)
        .ok_or_else(|| format!("missing value for {option}"))?;
    if value.starts_with("--") || value == "-x" {
        return Err(format!("missing value for {option}"));
    }
    Ok(value)
}

fn parse_next_usize(args: &[String], index: &mut usize, option: &str) -> Result<usize, String> {
    let value = option_value(args, *index, option)?;
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("{option} requires a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{option} requires a positive integer"));
    }
    *index += 2;
    Ok(parsed)
}

fn parse_next_u32(args: &[String], index: &mut usize, option: &str) -> Result<u32, String> {
    let value = option_value(args, *index, option)?;
    let parsed = value
        .parse::<u32>()
        .map_err(|_| format!("{option} requires a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{option} requires a positive integer"));
    }
    *index += 2;
    Ok(parsed)
}

fn parse_verbosity(value: &str) -> Result<Verbosity, String> {
    match value {
        "error" => Ok(Verbosity::Error),
        "info" => Ok(Verbosity::Info),
        "debug" => Ok(Verbosity::Debug),
        "trace" => Ok(Verbosity::Trace),
        _ => Err(format!("unsupported verbosity: {value}")),
    }
}

fn parse_exclude(value: &str) -> Result<RelPath, String> {
    if value.is_empty() {
        return Err("invalid exclude path: empty value".to_string());
    }
    RelPath::new(value.to_string()).map_err(|_| format!("invalid exclude path: {value}"))
}

fn parse_peer_operand(operand: &str, env: &CliParseEnv) -> Result<PeerSpec, String> {
    if operand.is_empty() {
        return Err("invalid peer operand: empty value".to_string());
    }

    let (role, body) = match operand.as_bytes()[0] {
        b'+' => (PeerRole::Canon, &operand[1..]),
        b'-' => (PeerRole::Subordinate, &operand[1..]),
        _ => (PeerRole::Normal, operand),
    };

    if body.is_empty() {
        return Err(format!("invalid peer operand: {operand}"));
    }

    let url_texts = if body.starts_with('[') || body.ends_with(']') {
        if !(body.starts_with('[') && body.ends_with(']')) {
            return Err(format!("invalid fallback group: {operand}"));
        }
        let inner = &body[1..body.len() - 1];
        if inner.is_empty() {
            return Err(format!("invalid fallback group: {operand}"));
        }
        inner
            .split(',')
            .map(str::trim)
            .map(|url| {
                if url.is_empty() || url.starts_with('+') || url.starts_with('-') {
                    Err(format!("invalid fallback URL in peer operand: {operand}"))
                } else {
                    Ok(url.to_string())
                }
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![body.to_string()]
    };

    let urls = url_texts
        .iter()
        .map(|url| parse_peer_url(url, env))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PeerSpec { role, urls })
}

fn parse_peer_url(input: &str, env: &CliParseEnv) -> Result<PeerUrl, String> {
    let (without_query, timeout_conn, timeout_idle) = split_query(input)?;
    let scheme = detect_scheme(without_query);

    match scheme.as_deref() {
        Some("sftp") => parse_sftp_url(without_query, timeout_conn, timeout_idle, env),
        Some("file") => parse_file_url(without_query, timeout_conn, timeout_idle, env),
        Some(other) => Err(format!("unsupported URL scheme: {other}")),
        None if has_malformed_supported_scheme(without_query) => {
            Err(format!("unsupported URL form: {without_query}"))
        }
        None => parse_bare_file_url(without_query, timeout_conn, timeout_idle, env),
    }
}

fn split_query(input: &str) -> Result<(&str, Option<u32>, Option<u32>), String> {
    let Some((base, query)) = input.split_once('?') else {
        return Ok((input, None, None));
    };
    if base.is_empty() {
        return Err("invalid URL: empty URL before query string".to_string());
    }
    if query.is_empty() {
        return Err(format!("empty query string in URL: {input}"));
    }

    let mut timeout_conn = None;
    let mut timeout_idle = None;
    for pair in query.split('&') {
        let (name, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("missing value for URL query parameter: {pair}"))?;
        if value.is_empty() {
            return Err(format!("missing value for URL query parameter: {name}"));
        }
        let parsed = value
            .parse::<u32>()
            .map_err(|_| format!("URL query parameter {name} requires a positive integer"))?;
        if parsed == 0 {
            return Err(format!(
                "URL query parameter {name} requires a positive integer"
            ));
        }
        match name {
            "timeout-conn" => {
                if timeout_conn.replace(parsed).is_some() {
                    return Err("duplicate URL query parameter: timeout-conn".to_string());
                }
            }
            "timeout-idle" => {
                if timeout_idle.replace(parsed).is_some() {
                    return Err("duplicate URL query parameter: timeout-idle".to_string());
                }
            }
            _ => return Err(format!("unsupported URL query parameter: {name}")),
        }
    }

    Ok((base, timeout_conn, timeout_idle))
}

fn detect_scheme(input: &str) -> Option<String> {
    let (scheme, _) = input.split_once("://")?;
    if scheme
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.')
    {
        Some(scheme.to_ascii_lowercase())
    } else {
        None
    }
}

fn has_malformed_supported_scheme(input: &str) -> bool {
    match input.split_once(':') {
        Some((scheme, _)) => matches!(scheme.to_ascii_lowercase().as_str(), "file" | "sftp"),
        None => false,
    }
}

fn parse_file_url(
    input: &str,
    timeout_conn: Option<u32>,
    timeout_idle: Option<u32>,
    env: &CliParseEnv,
) -> Result<PeerUrl, String> {
    if input.len() < "file://".len() {
        return Err(format!("invalid file URL: {input}"));
    }
    let mut path = &input["file://".len()..];
    if path.starts_with("localhost/") {
        path = &path["localhost".len()..];
    }
    normalized_file_url(path, timeout_conn, timeout_idle, env)
}

fn parse_bare_file_url(
    input: &str,
    timeout_conn: Option<u32>,
    timeout_idle: Option<u32>,
    env: &CliParseEnv,
) -> Result<PeerUrl, String> {
    normalized_file_url(input, timeout_conn, timeout_idle, env)
}

fn normalized_file_url(
    input_path: &str,
    timeout_conn: Option<u32>,
    timeout_idle: Option<u32>,
    env: &CliParseEnv,
) -> Result<PeerUrl, String> {
    if input_path.is_empty() {
        return Err("file URL path is empty".to_string());
    }
    let decoded = percent_decode_unreserved(input_path);
    let local_path = strip_file_url_drive_prefix(&decoded);
    let collapsed = collapse_slashes(local_path);
    let trimmed = trim_trailing_slash(&collapsed);
    let absolute = absolute_local_path(&env.current_dir, trimmed);
    let path = normalize_file_identity_path(&absolute.to_string_lossy().replace('\\', "/"));
    let identity = format!("file://{path}");

    Ok(PeerUrl {
        scheme: "file".to_string(),
        username: None,
        password: None,
        host: None,
        port: None,
        path,
        identity,
        timeout_conn,
        timeout_idle,
    })
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

fn absolute_local_path(base: &Path, path: &str) -> PathBuf {
    let path_buf = PathBuf::from(path);
    if path_buf.is_absolute() || path.starts_with('/') || is_windows_drive_path(path) {
        path_buf
    } else {
        base.join(path_buf)
    }
}

fn is_windows_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn normalize_file_identity_path(path: &str) -> String {
    let collapsed = collapse_slashes(path);
    let trimmed = trim_trailing_slash(&collapsed);
    let (prefix, rest) = file_path_prefix(trimmed);
    let mut segments: Vec<&str> = Vec::new();

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

fn parse_sftp_url(
    input: &str,
    timeout_conn: Option<u32>,
    timeout_idle: Option<u32>,
    env: &CliParseEnv,
) -> Result<PeerUrl, String> {
    if input.len() < "sftp://".len() {
        return Err(format!("invalid SFTP URL: {input}"));
    }
    let rest = &input["sftp://".len()..];
    let slash = rest
        .find('/')
        .ok_or_else(|| format!("SFTP URL requires an absolute path: {input}"))?;
    let authority = &rest[..slash];
    let raw_path = &rest[slash..];
    if authority.is_empty() || raw_path.is_empty() {
        return Err(format!("invalid SFTP URL: {input}"));
    }

    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((userinfo, hostport)) => (Some(userinfo), hostport),
        None => (None, authority),
    };

    let (username, password) = match userinfo {
        Some(value) if value.is_empty() => return Err(format!("invalid SFTP userinfo: {input}")),
        Some(value) => match value.split_once(':') {
            Some((user, password)) => {
                if user.is_empty() {
                    return Err(format!("invalid SFTP userinfo: {input}"));
                }
                (
                    Some(percent_decode_userinfo(user)),
                    Some(percent_decode_userinfo(password)),
                )
            }
            None => (Some(percent_decode_userinfo(value)), None),
        },
        None => (Some(env.current_user.clone()), None),
    };

    let (host, port) = parse_host_port(hostport, input)?;
    let host = host.to_ascii_lowercase();
    let decoded_path = percent_decode_unreserved(raw_path);
    let collapsed_path = collapse_slashes(&decoded_path);
    let path = trim_trailing_slash(&collapsed_path).to_string();
    let identity_port = port
        .filter(|port| *port != 22)
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let identity = format!(
        "sftp://{}@{}{}{}",
        username.as_deref().unwrap_or(&env.current_user),
        host,
        identity_port,
        path
    );

    Ok(PeerUrl {
        scheme: "sftp".to_string(),
        username,
        password,
        host: Some(host),
        port: port.filter(|port| *port != 22),
        path,
        identity,
        timeout_conn,
        timeout_idle,
    })
}

fn parse_host_port(hostport: &str, input: &str) -> Result<(String, Option<u16>), String> {
    if hostport.is_empty() {
        return Err(format!("SFTP URL host is empty: {input}"));
    }

    if let Some((host, port_text)) = hostport.rsplit_once(':') {
        if host.is_empty() || port_text.is_empty() {
            return Err(format!("invalid SFTP host or port: {input}"));
        }
        if port_text.chars().all(|ch| ch.is_ascii_digit()) {
            let port = port_text
                .parse::<u16>()
                .map_err(|_| format!("invalid SFTP port: {port_text}"))?;
            if port == 0 {
                return Err(format!("invalid SFTP port: {port_text}"));
            }
            Ok((percent_decode_unreserved(host), Some(port)))
        } else {
            Err(format!("invalid SFTP port: {port_text}"))
        }
    } else {
        Ok((percent_decode_unreserved(hostport), None))
    }
}

fn collapse_slashes(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut previous_slash = false;
    for ch in input.chars() {
        if ch == '/' {
            if !previous_slash {
                output.push(ch);
            }
            previous_slash = true;
        } else {
            output.push(ch);
            previous_slash = false;
        }
    }
    output
}

fn trim_trailing_slash(input: &str) -> &str {
    if input == "/" || is_windows_drive_root(input) {
        input
    } else {
        input.trim_end_matches('/')
    }
}

fn is_windows_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

fn percent_decode_unreserved(input: &str) -> String {
    percent_decode_with(input, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
    })
}

fn percent_decode_userinfo(input: &str) -> String {
    percent_decode_with(input, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'@' | b':')
    })
}

fn percent_decode_with(input: &str, allow: impl Fn(u8) -> bool) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                let decoded = high * 16 + low;
                if allow(decoded) {
                    output.push(decoded as char);
                    index += 3;
                    continue;
                }
            }
        }
        output.push(bytes[index] as char);
        index += 1;
    }
    output
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn os_string_to_string(value: OsString) -> String {
    value
        .into_string()
        .unwrap_or_else(|value| value.as_os_str().to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> CliParseEnv {
        CliParseEnv {
            current_dir: PathBuf::from("C:/work"),
            current_user: "alice".to_string(),
        }
    }

    #[test]
    fn parses_options_peers_and_excludes() {
        let result = parse_invocation(
            [
                "--dry-run",
                "--max-copies",
                "2",
                "-x",
                "ignored/path",
                "+left",
                "sftp://host/root?timeout-conn=4",
            ],
            &env(),
        );
        let CliInvocation::Run(request) = result else {
            panic!("expected valid run");
        };
        assert!(request.config.dry_run);
        assert_eq!(request.config.max_copies, 2);
        assert_eq!(request.peers[0].role, PeerRole::Canon);
        assert_eq!(request.excludes[0].as_str(), "ignored/path");
        assert_eq!(request.peers[1].urls[0].username.as_deref(), Some("alice"));
        assert_eq!(request.peers[1].urls[0].timeout_conn, Some(4));
    }

    #[test]
    fn rejects_invalid_exclude() {
        let result = parse_invocation(["-x", "../bad", "left", "right"], &env());
        assert!(matches!(result, CliInvocation::Invalid { .. }));
    }

    #[test]
    fn normalizes_file_url_drive_paths() {
        let result = parse_invocation(["file:///C:/left", "file://localhost/C:/right"], &env());
        let CliInvocation::Run(request) = result else {
            panic!("expected valid run");
        };
        assert_eq!(request.peers[0].urls[0].identity, "file://C:/left");
        assert_eq!(request.peers[1].urls[0].identity, "file://C:/right");
    }

    #[test]
    fn rejects_empty_sftp_user_with_inline_password() {
        let result = parse_invocation([":pw@host/root", "right"], &env());
        let CliInvocation::Run(request) = result else {
            panic!("bare local path should remain valid");
        };
        assert_eq!(request.peers[0].urls[0].scheme, "file");

        let result = parse_invocation(["sftp://:pw@host/root", "right"], &env());
        assert!(matches!(result, CliInvocation::Invalid { .. }));
    }
}
