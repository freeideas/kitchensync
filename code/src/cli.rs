//! Command-line parsing: turns `std::env::args()` into a `config::Parsed`.
//! See specs/sync.md "Command Line", "Canon Peer", "Subordinate Peer", and
//! "Startup" step 1; specs/help.md.

use crate::config::{Config, Parsed, PeerSpec, PeerUrl, Role, Scheme, Mode};
use crate::output::Verbosity;

/// Exact text from specs/help.md, byte-for-byte (no BOM), trailing newline.
pub const HELP_TEXT: &str = r#"Usage: kitchensync [options] <peer> <peer> [<peer>...]

Synchronize file trees across multiple peers.

Running with no arguments (or --help, -h, /?) prints this help. See the specs for full behavior.

Peers:
  /path or c:\path                 Local path (same as file://)
  sftp://user@host/path            Remote over SSH
  sftp://user@host:port/path       Non-standard SSH port
  sftp://host/path                 Remote over SSH, current OS user
  sftp://user:password@host/path   Inline password (prefer SSH keys)

Prefix modifiers:
  +<peer>                          Canon - this peer's state wins all conflicts
  -<peer>                          Subordinate - overwritten to match the group

Fallback URLs (multiple paths to the same data):
  [url1,url2,...]                  Try in order, first that connects wins
  +[url1,url2,...]                 Canon peer with fallbacks
  -[url1,url2,...]                 Subordinate peer with fallbacks

Per-URL settings (query string, inside quotes):
  "sftp://host/path?timeout-conn=60"     Connection timeout for this URL
  "sftp://host/path?timeout-idle=10"     SFTP idle keep-alive TTL for this URL
  "sftp://host/path?timeout-conn=60&timeout-idle=10"  Combine multiple

Options:
  --dry-run          Read-only and plan, but make no peer changes
  --parallel N       Files copied at the same time (default: 5)
  --retries-copy N   Give up copying after this many tries (default: 3)
  --retries-list N   Give up listing after this many tries (default: 3)
  --timeout-conn N   SSH handshake timeout in seconds (default: 30)
  --timeout-idle N   SFTP idle keep-alive TTL in seconds (default: 30)
  --verbosity LEVEL  Verbosity: error, info, debug, trace (default: info)
  --rollback TS      Roll the given peers back to timestamp TS, then exit
  --undo             Roll the given peers back to just before their newest run
  -x PATTERN         Exclude like .gitignore (or @file of patterns); repeatable
  --keep-bak-days N  Delete displaced files (BAK/) after N days (default: 90)
  --keep-del-days N  Forget deletion records after N days (default: 180)

Quick start:
  kitchensync +c:/photos sftp://user@host/photos      First sync (c: is canon)
  kitchensync c:/photos sftp://host/photos            Bidirectional
  kitchensync c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate
  kitchensync c:/photos "sftp://user:p%40ss@host/photos"  Inline password

Without + on a first sync, peers are merged both ways and nothing is deleted.
Use + to make one peer's contents win instead.

Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.

Displaced files are recoverable from nearby:
  .kitchensync/BAK/ directories (kept for --keep-bak-days days).
"#;

/// Arguments that print the help screen and exit 0, wherever they appear.
const HELP_FLAGS: [&str; 3] = ["--help", "-h", "/?"];

/// Parse the full command line (argv, without the program name).
pub fn parse(args: &[String]) -> Parsed {
    if args.is_empty() || args.iter().any(|a| HELP_FLAGS.contains(&a.as_str())) {
        return Parsed::Help;
    }

    let mut cfg = Config::default();
    // Peers are collected as (role, remainder-after-prefix) and resolved
    // into `PeerSpec`s only after the option loop, so peer-count checks can
    // run before we bother parsing any URLs.
    let mut peer_specs: Vec<(Role, String)> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();

        if a == "--dry-run" {
            cfg.dry_run = true;
            i += 1;
            continue;
        }
        if a == "--undo" {
            if cfg.mode != Mode::Sync {
                return Parsed::Error("--rollback and --undo cannot be combined".to_string());
            }
            cfg.mode = Mode::Undo;
            i += 1;
            continue;
        }
        if a == "--rollback" {
            if cfg.mode != Mode::Sync {
                return Parsed::Error("--rollback and --undo cannot be combined".to_string());
            }
            let Some(v) = args.get(i + 1) else {
                return Parsed::Error("missing value for --rollback".to_string());
            };
            let Some(ts) = crate::util::parse_time(v) else {
                return Parsed::Error(format!("invalid timestamp for --rollback: {v} (expected YYYY-MM-DD_HH-mm-ss_ffffffZ)"));
            };
            cfg.mode = Mode::Rollback(ts);
            i += 2;
            continue;
        }

        if a.starts_with("--") {
            match a {
                "--parallel" => match take_positive_int(args, i, a) {
                    Ok((n, next)) => {
                        cfg.parallel = n as usize;
                        i = next;
                    }
                    Err(e) => return Parsed::Error(e),
                },
                "--retries-copy" => match take_positive_int(args, i, a) {
                    Ok((n, next)) => match u32::try_from(n) {
                        Ok(v) => {
                            cfg.retries_copy = v;
                            i = next;
                        }
                        Err(_) => {
                            return Parsed::Error(format!("invalid value for {}: {}", a, n));
                        }
                    },
                    Err(e) => return Parsed::Error(e),
                },
                "--retries-list" => match take_positive_int(args, i, a) {
                    Ok((n, next)) => match u32::try_from(n) {
                        Ok(v) => {
                            cfg.retries_list = v;
                            i = next;
                        }
                        Err(_) => {
                            return Parsed::Error(format!("invalid value for {}: {}", a, n));
                        }
                    },
                    Err(e) => return Parsed::Error(e),
                },
                "--timeout-conn" => match take_positive_int(args, i, a) {
                    Ok((n, next)) => {
                        cfg.timeout_conn = n;
                        i = next;
                    }
                    Err(e) => return Parsed::Error(e),
                },
                "--timeout-idle" => match take_positive_int(args, i, a) {
                    Ok((n, next)) => {
                        cfg.timeout_idle = n;
                        i = next;
                    }
                    Err(e) => return Parsed::Error(e),
                },
                "--keep-bak-days" => match take_positive_int(args, i, a) {
                    Ok((n, next)) => {
                        cfg.keep_bak_days = n;
                        i = next;
                    }
                    Err(e) => return Parsed::Error(e),
                },
                "--keep-del-days" => match take_positive_int(args, i, a) {
                    Ok((n, next)) => {
                        cfg.keep_del_days = n;
                        i = next;
                    }
                    Err(e) => return Parsed::Error(e),
                },
                "--verbosity" => {
                    let val = match args.get(i + 1) {
                        Some(v) => v,
                        None => return Parsed::Error(format!("missing value for {}", a)),
                    };
                    match Verbosity::parse(val) {
                        Some(v) => cfg.verbosity = v,
                        None => {
                            return Parsed::Error(format!(
                                "invalid value for --verbosity: {}",
                                val
                            ));
                        }
                    }
                    i += 2;
                }
                _ => return Parsed::Error(format!("unrecognized option: {}", a)),
            }
            continue;
        }

        if a == "-x" {
            let val = match args.get(i + 1) {
                Some(v) => v,
                None => return Parsed::Error("missing value for -x".to_string()),
            };
            if let Err(e) = add_exclude(&mut cfg.ignore, val) {
                return Parsed::Error(e);
            }
            i += 2;
            continue;
        }

        // Anything else is a peer: `+peer` is canon, `-peer` is subordinate,
        // everything else (including bare `-`-prefixed non-flag strings like
        // `-/mnt/usb` or `-[url1,url2]`) is a peer too.
        let (role, rest) = if let Some(r) = a.strip_prefix('+') {
            (Role::Canon, r)
        } else if let Some(r) = a.strip_prefix('-') {
            (Role::Subordinate, r)
        } else {
            (Role::Normal, a)
        };
        peer_specs.push((role, rest.to_string()));
        i += 1;
    }

    if cfg.mode != Mode::Sync {
        if peer_specs.is_empty() {
            return Parsed::Error("at least one peer is required".to_string());
        }
        if peer_specs.iter().any(|(r, _)| *r != Role::Normal) {
            return Parsed::Error("+ and - prefixes are not allowed with --rollback or --undo".to_string());
        }
    } else if peer_specs.len() < 2 {
        return Parsed::Error("at least two peers are required".to_string());
    }
    let canon_count = peer_specs.iter().filter(|(r, _)| *r == Role::Canon).count();
    if canon_count > 1 {
        return Parsed::Error("only one canon (+) peer is allowed".to_string());
    }

    let mut peers = Vec::with_capacity(peer_specs.len());
    for (role, rest) in peer_specs {
        let url_strs = match split_fallback_urls(&rest) {
            Ok(v) => v,
            Err(e) => return Parsed::Error(e),
        };
        let mut urls = Vec::with_capacity(url_strs.len());
        for u in &url_strs {
            match parse_url(u) {
                Ok(pu) => urls.push(pu),
                Err(e) => return Parsed::Error(e),
            }
        }
        peers.push(PeerSpec { role, urls });
    }
    cfg.peers = peers;

    Parsed::Run(cfg)
}

/// Read `<flag> <value>` at `args[i]`/`args[i+1]`, requiring a positive
/// integer value. Returns the parsed value and the index just past it.
fn take_positive_int(args: &[String], i: usize, name: &str) -> Result<(u64, usize), String> {
    let val = args
        .get(i + 1)
        .ok_or_else(|| format!("missing value for {}", name))?;
    let n = parse_positive_u64(name, val)?;
    Ok((n, i + 2))
}

/// Parse a positive integer (>= 1) from a string, for both flag values and
/// URL query values.
fn parse_positive_u64(name: &str, val: &str) -> Result<u64, String> {
    match val.parse::<u64>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(format!("invalid value for {}: {}", name, val)),
    }
}

/// Validate a `-x` relative-path argument.
/// See specs/sync.md "Command-Line Excludes".
/// `-x PATTERN` adds one gitignore-style pattern; `-x @FILE` reads a file of them.
fn add_exclude(set: &mut crate::ignore::IgnoreSet, val: &str) -> Result<(), String> {
    if val.is_empty() || val.contains('\0') {
        return Err(format!("invalid value for -x: {val:?}"));
    }
    if let Some(file) = val.strip_prefix('@') {
        let text = std::fs::read_to_string(file).map_err(|e| format!("cannot read -x file {file}: {e}"))?;
        return set.add_text(&text).map_err(|e| format!("invalid pattern in {file}: {e}"));
    }
    match set.add_line(val) {
        Ok(()) if set.is_empty() => Err(format!("invalid value for -x: {val:?} (comment or blank)")),
        Ok(()) => Ok(()),
        Err(e) => Err(format!("invalid value for -x: {e}")),
    }
}

/// Split a peer's URL body into one or more raw URL strings, handling the
/// `[url1,url2,...]` fallback-list syntax. See specs/sync.md "Fallback URLs".
fn split_fallback_urls(rest: &str) -> Result<Vec<String>, String> {
    let has_open = rest.starts_with('[');
    let has_close = rest.ends_with(']');
    if has_open != has_close {
        return Err(format!("unbalanced brackets in peer: {}", rest));
    }
    if !has_open {
        if rest.is_empty() {
            return Err("empty peer URL".to_string());
        }
        return Ok(vec![rest.to_string()]);
    }
    let inner = &rest[1..rest.len() - 1];
    if inner.contains('[') || inner.contains(']') {
        return Err(format!("unbalanced brackets in peer: {}", rest));
    }
    if inner.is_empty() {
        return Err(format!("empty fallback URL list: {}", rest));
    }
    let parts: Vec<String> = inner.split(',').map(|s| s.to_string()).collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err(format!("empty URL in fallback list: {}", rest));
    }
    Ok(parts)
}

/// Detect a `scheme://` prefix. Returns the lowercase scheme name if `raw`
/// starts with one. A Windows drive letter (`c:/...`, `c:\...`) is not a
/// scheme: it lacks the `//` that follows a real scheme's colon.
fn detect_scheme(raw: &str) -> Option<String> {
    let idx = raw.find("://")?;
    let candidate = &raw[..idx];
    if candidate.is_empty() {
        return None;
    }
    let mut chars = candidate.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
        return None;
    }
    Some(candidate.to_ascii_lowercase())
}

/// Parse one URL/path into a `PeerUrl`, filling in defaults and computing
/// the normalized identity string. See specs/sync.md "URL Schemes" and
/// specs/database.md "URL Normalization".
pub fn parse_url(raw: &str) -> Result<PeerUrl, String> {
    match detect_scheme(raw) {
        Some(scheme) => match scheme.as_str() {
            "sftp" => parse_sftp_url(raw),
            "file" => parse_file_url(raw),
            other => Err(format!("unsupported URL scheme: {}", other)),
        },
        None => parse_file_url(raw),
    }
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// `sftp://[user[:password]@]host[:port]/path[?query]`
fn parse_sftp_url(raw: &str) -> Result<PeerUrl, String> {
    let rest = &raw[7..]; // strip "sftp://" (7 bytes, same length as "file://")
    let (before_query, query) = match rest.find('?') {
        Some(idx) => (&rest[..idx], Some(&rest[idx + 1..])),
        None => (rest, None),
    };
    let slash_idx = before_query
        .find('/')
        .ok_or_else(|| format!("invalid sftp url (missing path): {}", raw))?;
    let authority = &before_query[..slash_idx];
    let path_part = &before_query[slash_idx..];
    if authority.is_empty() {
        return Err(format!("invalid sftp url (missing host): {}", raw));
    }

    let (userinfo, hostport) = match authority.rfind('@') {
        Some(idx) => (Some(&authority[..idx]), &authority[idx + 1..]),
        None => (None, authority),
    };

    let (host_raw, port) = match hostport.rfind(':') {
        Some(idx) => {
            let port_str = &hostport[idx + 1..];
            let port: u16 = port_str
                .parse()
                .map_err(|_| format!("invalid port in sftp url: {}", raw))?;
            (&hostport[..idx], port)
        }
        None => (hostport, 22u16),
    };
    if host_raw.is_empty() {
        return Err(format!("invalid sftp url (missing host): {}", raw));
    }
    let host = host_raw.to_ascii_lowercase();

    let (user, password) = match userinfo {
        Some(ui) => match ui.find(':') {
            Some(idx) => (
                percent_decode(&ui[..idx]),
                Some(percent_decode(&ui[idx + 1..])),
            ),
            None => (percent_decode(ui), None),
        },
        None => (current_os_user(), None),
    };

    let decoded_path = percent_decode(path_part);
    let path = collapse_and_trim_path(&decoded_path);

    let mut timeout_conn = None;
    let mut timeout_idle = None;
    if let Some(q) = query {
        for pair in q.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = match pair.find('=') {
                Some(idx) => (&pair[..idx], &pair[idx + 1..]),
                None => (pair, ""),
            };
            match k {
                "timeout-conn" => timeout_conn = Some(parse_positive_u64("timeout-conn", v)?),
                "timeout-idle" => timeout_idle = Some(parse_positive_u64("timeout-idle", v)?),
                _ => return Err(format!("invalid URL query parameter: {}", k)),
            }
        }
    }

    let normalized = if port == 22 {
        format!("sftp://{}@{}{}", user, host, path)
    } else {
        format!("sftp://{}@{}:{}{}", user, host, port, path)
    };

    Ok(PeerUrl {
        scheme: Scheme::Sftp,
        normalized,
        path,
        user: Some(user),
        password,
        host,
        port,
        timeout_conn,
        timeout_idle,
    })
}

/// Collapse consecutive slashes and remove a trailing slash, keeping at
/// least "/". `p` is assumed to start with "/".
fn collapse_and_trim_path(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    let mut prev_slash = false;
    for c in p.chars() {
        if c == '/' {
            if prev_slash {
                continue;
            }
            prev_slash = true;
            out.push('/');
        } else {
            prev_slash = false;
            out.push(c);
        }
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

/// True if `s` looks like a Windows drive-letter path start, e.g. "c:/x" or
/// "c:" - a single ASCII letter followed by ':'.
fn is_drive_letter_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Split an absolute path into its root marker ("/" for Unix, "c:" for a
/// Windows drive) and the remainder with no leading slash.
fn split_root(full: &str) -> (String, &str) {
    if let Some(rest) = full.strip_prefix('/') {
        ("/".to_string(), rest)
    } else {
        let drive = &full[0..2];
        let rest = full[2..].strip_prefix('/').unwrap_or(&full[2..]);
        (drive.to_string(), rest)
    }
}

/// Bare path or `file://` URL. Resolves relative paths against cwd and
/// lexically normalizes `.`/`..` segments (no symlink resolution).
fn parse_file_url(raw: &str) -> Result<PeerUrl, String> {
    let has_scheme = raw.len() >= 7 && raw[..7].eq_ignore_ascii_case("file://");
    let rem = if has_scheme { &raw[7..] } else { raw };

    let mut s = rem.replace('\\', "/");
    // "file:///c:/x" -> after stripping "file://" we have "/c:/x"; drop the
    // extra leading slash in front of a drive letter.
    if let Some(after_slash) = s.strip_prefix('/') {
        if is_drive_letter_path(after_slash) {
            s = after_slash.to_string();
        }
    }
    if s.is_empty() {
        return Err(format!("invalid file url (empty path): {}", raw));
    }

    let is_abs = s.starts_with('/') || is_drive_letter_path(&s);
    let full = if is_abs {
        s
    } else {
        let cwd = std::env::current_dir()
            .map_err(|e| format!("cannot determine current directory: {}", e))?;
        let cwd_str = cwd.to_string_lossy().replace('\\', "/");
        format!("{}/{}", cwd_str.trim_end_matches('/'), s)
    };

    let (root, rest) = split_root(&full);
    let mut stack: Vec<&str> = Vec::new();
    for seg in rest.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            seg => stack.push(seg),
        }
    }
    let joined = stack.join("/");
    let path = if root == "/" {
        format!("/{}", joined)
    } else if joined.is_empty() {
        root.clone()
    } else {
        format!("{}/{}", root, joined)
    };

    let normalized_path = if path.starts_with('/') {
        path.clone()
    } else {
        format!("/{}", path)
    };
    let normalized = format!("file://{}", normalized_path);

    Ok(PeerUrl {
        scheme: Scheme::File,
        normalized,
        path,
        user: None,
        password: None,
        host: String::new(),
        port: 0,
        timeout_conn: None,
        timeout_idle: None,
    })
}

/// The current OS user, for SFTP URLs with no explicit username.
fn current_os_user() -> String {
    if let Ok(u) = std::env::var("USER") {
        if !u.is_empty() {
            return u;
        }
    }
    if let Ok(u) = std::env::var("USERNAME") {
        if !u.is_empty() {
            return u;
        }
    }
    "user".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn help_on_empty_args() {
        match parse(&[]) {
            Parsed::Help => {}
            _ => panic!("expected Help"),
        }
    }

    #[test]
    fn help_on_help_flags() {
        for flag in ["--help", "-h", "/?"] {
            match parse(&args(&["/tmp/a", flag, "/tmp/b"])) {
                Parsed::Help => {}
                other => panic!("expected Help for {flag}, got {:?}", debug_kind(&other)),
            }
        }
    }

    #[test]
    fn help_text_shape() {
        assert!(HELP_TEXT.starts_with("Usage: kitchensync"));
        assert!(HELP_TEXT.ends_with('\n'));
    }

    #[test]
    fn error_on_one_peer() {
        match parse(&args(&["/tmp/a"])) {
            Parsed::Error(msg) => assert_eq!(msg, "at least two peers are required"),
            other => panic!("expected Error, got {:?}", debug_kind(&other)),
        }
    }

    #[test]
    fn error_on_two_canons() {
        match parse(&args(&["+/tmp/a", "+/tmp/b"])) {
            Parsed::Error(msg) => assert_eq!(msg, "only one canon (+) peer is allowed"),
            other => panic!("expected Error, got {:?}", debug_kind(&other)),
        }
    }

    #[test]
    fn subordinate_dash_path_is_a_peer_not_a_flag() {
        match parse(&args(&["/tmp/a", "-/mnt/usb"])) {
            Parsed::Run(cfg) => {
                assert_eq!(cfg.peers.len(), 2);
                assert_eq!(cfg.peers[1].role, Role::Subordinate);
                assert_eq!(cfg.peers[1].urls[0].path, "/mnt/usb");
            }
            other => panic!("expected Run, got {:?}", debug_kind(&other)),
        }
    }

    #[test]
    fn bracket_fallback_parsing() {
        match parse(&args(&["/tmp/a", "[/tmp/b,/tmp/c]"])) {
            Parsed::Run(cfg) => {
                assert_eq!(cfg.peers[1].role, Role::Normal);
                assert_eq!(cfg.peers[1].urls.len(), 2);
                assert_eq!(cfg.peers[1].urls[0].path, "/tmp/b");
                assert_eq!(cfg.peers[1].urls[1].path, "/tmp/c");
            }
            other => panic!("expected Run, got {:?}", debug_kind(&other)),
        }
    }

    #[test]
    fn canon_bracket_fallback() {
        match parse(&args(&["/tmp/a", "+[/tmp/b,/tmp/c]"])) {
            Parsed::Run(cfg) => {
                assert_eq!(cfg.peers[1].role, Role::Canon);
                assert_eq!(cfg.peers[1].urls.len(), 2);
            }
            other => panic!("expected Run, got {:?}", debug_kind(&other)),
        }
    }

    #[test]
    fn per_url_timeout_parsing() {
        let u = parse_url("sftp://host/path?timeout-conn=60&timeout-idle=10").unwrap();
        assert_eq!(u.timeout_conn, Some(60));
        assert_eq!(u.timeout_idle, Some(10));
    }

    #[test]
    fn parallel_rejected_in_query() {
        let e = parse_url("sftp://host/path?parallel=5").unwrap_err();
        assert!(e.contains("parallel"), "unexpected message: {}", e);
    }

    #[test]
    fn normalization_examples_from_database_spec() {
        let user = current_os_user();

        let u1 = parse_url("SFTP://Host:22/path/").unwrap();
        assert_eq!(u1.normalized, format!("sftp://{}@host/path", user));

        let u2 = parse_url("sftp://host//docs/").unwrap();
        assert_eq!(u2.normalized, format!("sftp://{}@host/docs", user));

        let u3 = parse_url("sftp://host/path?timeout-conn=60").unwrap();
        assert_eq!(u3.normalized, format!("sftp://{}@host/path", user));
    }

    #[test]
    fn exclude_path_validation() {
        let mut set = crate::ignore::IgnoreSet::new();
        assert!(add_exclude(&mut set, "docs/readme.txt").is_ok());
        assert!(add_exclude(&mut set, "*.tmp").is_ok());
        assert!(add_exclude(&mut set, "!keep.tmp").is_ok());
        assert!(add_exclude(&mut set, "").is_err());
        assert!(add_exclude(&mut set, "@/nonexistent/ignore/file").is_err());
        assert!(set.is_excluded("a/b.tmp", false));
        assert!(!set.is_excluded("keep.tmp", false));
    }

    /// Small helper so panic messages are readable without requiring Config
    /// or Parsed to implement Debug.
    fn debug_kind(p: &Parsed) -> &'static str {
        match p {
            Parsed::Help => "Help",
            Parsed::Error(_) => "Error",
            Parsed::Run(_) => "Run",
        }
    }
}
