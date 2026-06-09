use std::sync::Arc;
use crate::api::*;

const HELP_TEXT: &str = r#"Usage: kitchensync [options] <peer> <peer> [<peer>...]

Synchronize file trees across multiple peers.

Running with no arguments prints this help. See the specs for full behavior.

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
  --max-copies N     Max active file copies across the whole run (default: 10)
  --retries-copy N   Give up copying after this many tries (default: 3)
  --retries-list N   Give up listing after this many tries (default: 3)
  --timeout-conn N   SSH handshake timeout in seconds (default: 30)
  --timeout-idle N   SFTP idle keep-alive TTL in seconds (default: 30)
  --verbosity LEVEL  Verbosity: error, info, debug, trace (default: info)
  -x RELPATH         Exclude relative slash path from sync; repeatable
  --keep-tmp-days N  Delete stale TMP staging after N days (default: 2)
  --keep-bak-days N  Delete displaced files (BAK/) after N days (default: 90)
  --keep-del-days N  Forget deletion records after N days (default: 180)

Quick start:
  kitchensync +c:/photos sftp://user@host/photos      First sync (c: is canon)
  kitchensync c:/photos sftp://host/photos            Bidirectional
  kitchensync c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate
  kitchensync c:/photos "sftp://user:p%40ss@host/photos"  Inline password

Canon (+) is required on first sync when no peer has snapshot history.
After the first sync, bidirectional sync works without canon.

Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.

Displaced files are recoverable from nearby:
  .kitchensync/BAK/ directories (kept for --keep-bak-days days)."#;

struct CliImpl;

impl Cli for CliImpl {
    fn parse(&self, args: Vec<String>) -> CliOutcome {
        if args.is_empty() {
            return CliOutcome::Help;
        }
        match do_parse(args) {
            Ok(config) => CliOutcome::Run(config),
            Err(msg) => CliOutcome::Reject(msg),
        }
    }

    fn help_text(&self) -> String {
        HELP_TEXT.to_string()
    }
}

pub fn new() -> Arc<dyn Cli> {
    Arc::new(CliImpl)
}

fn do_parse(args: Vec<String>) -> Result<RunConfig, String> {
    let mut peers: Vec<Peer> = Vec::new();
    let mut excludes: Vec<String> = Vec::new();
    let mut dry_run = false;
    let mut max_copies: u32 = 10;
    let mut retries_copy: u32 = 3;
    let mut retries_list: u32 = 3;
    let mut timeout_conn: u32 = 30;
    let mut timeout_idle: u32 = 30;
    let mut verbosity = Verbosity::Info;
    let mut keep_tmp_days: u32 = 2;
    let mut keep_bak_days: u32 = 90;
    let mut keep_del_days: u32 = 180;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--dry-run" {
            dry_run = true;
        } else if arg == "--max-copies" {
            i += 1;
            max_copies = parse_positive_int(next_arg(&args, i, "--max-copies")?, "--max-copies")?;
        } else if arg == "--retries-copy" {
            i += 1;
            retries_copy =
                parse_positive_int(next_arg(&args, i, "--retries-copy")?, "--retries-copy")?;
        } else if arg == "--retries-list" {
            i += 1;
            retries_list =
                parse_positive_int(next_arg(&args, i, "--retries-list")?, "--retries-list")?;
        } else if arg == "--timeout-conn" {
            i += 1;
            timeout_conn =
                parse_positive_int(next_arg(&args, i, "--timeout-conn")?, "--timeout-conn")?;
        } else if arg == "--timeout-idle" {
            i += 1;
            timeout_idle =
                parse_positive_int(next_arg(&args, i, "--timeout-idle")?, "--timeout-idle")?;
        } else if arg == "--verbosity" {
            i += 1;
            verbosity = parse_verbosity(next_arg(&args, i, "--verbosity")?)?;
        } else if arg == "-x" {
            i += 1;
            let path = next_arg(&args, i, "-x")?.to_string();
            validate_exclude_path(&path)?;
            excludes.push(path);
        } else if arg == "--keep-tmp-days" {
            i += 1;
            keep_tmp_days =
                parse_positive_int(next_arg(&args, i, "--keep-tmp-days")?, "--keep-tmp-days")?;
        } else if arg == "--keep-bak-days" {
            i += 1;
            keep_bak_days =
                parse_positive_int(next_arg(&args, i, "--keep-bak-days")?, "--keep-bak-days")?;
        } else if arg == "--keep-del-days" {
            i += 1;
            keep_del_days =
                parse_positive_int(next_arg(&args, i, "--keep-del-days")?, "--keep-del-days")?;
        } else if arg.starts_with("--") {
            return Err(format!("unrecognized option: {}", arg));
        } else {
            peers.push(parse_peer(arg)?);
        }
        i += 1;
    }

    if peers.len() < 2 {
        return Err("at least two peers are required".to_string());
    }
    let canon_count = peers
        .iter()
        .filter(|p| matches!(p.role, PeerRole::Canon))
        .count();
    if canon_count > 1 {
        return Err("at most one canon (+) peer is allowed".to_string());
    }

    Ok(RunConfig {
        peers,
        excludes,
        options: GlobalOptions {
            dry_run,
            max_copies,
            retries_copy,
            retries_list,
            timeout_conn,
            timeout_idle,
            verbosity,
            keep_tmp_days,
            keep_bak_days,
            keep_del_days,
        },
    })
}

fn next_arg<'a>(args: &'a [String], i: usize, opt: &str) -> Result<&'a str, String> {
    if i >= args.len() {
        Err(format!("{} requires a value", opt))
    } else {
        Ok(&args[i])
    }
}

fn parse_positive_int(s: &str, name: &str) -> Result<u32, String> {
    let v: i64 = s
        .parse()
        .map_err(|_| format!("{} must be a positive integer, got: {}", name, s))?;
    if v <= 0 {
        return Err(format!("{} must be a positive integer, got: {}", name, s));
    }
    if v > u32::MAX as i64 {
        return Err(format!("{} must be a positive integer, got: {}", name, s));
    }
    Ok(v as u32)
}

fn parse_verbosity(s: &str) -> Result<Verbosity, String> {
    match s {
        "error" => Ok(Verbosity::Error),
        "info" => Ok(Verbosity::Info),
        "debug" => Ok(Verbosity::Debug),
        "trace" => Ok(Verbosity::Trace),
        other => Err(format!(
            "--verbosity must be one of: error, info, debug, trace; got: {}",
            other
        )),
    }
}

fn validate_exclude_path(path: &str) -> Result<(), String> {
    if path.contains('\0') {
        return Err(format!("exclude path must not contain NUL: {:?}", path));
    }
    if path.starts_with('/') {
        return Err(format!("exclude path must not start with /: {}", path));
    }
    if path.ends_with('/') {
        return Err(format!("exclude path must not end with /: {}", path));
    }
    if path.contains('\\') {
        return Err(format!(
            "exclude path must not contain backslash: {}",
            path
        ));
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(format!(
                "exclude path contains invalid segment in: {}",
                path
            ));
        }
    }
    Ok(())
}

fn parse_peer(arg: &str) -> Result<Peer, String> {
    let (role, rest) = if arg.starts_with('+') {
        (PeerRole::Canon, &arg[1..])
    } else if arg.starts_with('-') {
        (PeerRole::Subordinate, &arg[1..])
    } else {
        (PeerRole::Normal, arg)
    };

    let urls = if rest.starts_with('[') {
        if !rest.ends_with(']') {
            return Err(format!("malformed bracketed peer group: {}", arg));
        }
        let inner = &rest[1..rest.len() - 1];
        inner
            .split(',')
            .map(|u| parse_peer_url(u.trim()))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![parse_peer_url(rest)?]
    };

    Ok(Peer { role, urls })
}

fn parse_peer_url(url_str: &str) -> Result<PeerUrl, String> {
    let settings = parse_url_settings(url_str)?;
    Ok(PeerUrl {
        url: url_str.to_string(),
        settings,
    })
}

fn parse_url_settings(url: &str) -> Result<UrlSettings, String> {
    let mut timeout_conn = None;
    let mut timeout_idle = None;

    if let Some(q_pos) = url.find('?') {
        let query = &url[q_pos + 1..];
        for param in query.split('&') {
            if param.is_empty() {
                continue;
            }
            let (key, val) = match param.find('=') {
                Some(eq) => (&param[..eq], &param[eq + 1..]),
                None => (param, ""),
            };
            match key {
                "timeout-conn" => {
                    timeout_conn = Some(parse_positive_int(val, "timeout-conn")?);
                }
                "timeout-idle" => {
                    timeout_idle = Some(parse_positive_int(val, "timeout-idle")?);
                }
                "max-copies" => {
                    return Err(
                        "max-copies is not a valid per-URL parameter; use --max-copies".to_string(),
                    );
                }
                other => {
                    return Err(format!("unrecognized URL query parameter: {}", other));
                }
            }
        }
    }

    Ok(UrlSettings {
        timeout_conn,
        timeout_idle,
    })
}
