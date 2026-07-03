use crate::api::*;
use std::sync::Arc;

const HELP_TEXT: &str = "Usage: kitchensync [options] <peer> <peer> [<peer>...]\n\
\n\
Synchronize file trees across multiple peers.\n\
\n\
Running with no arguments prints this help. See the specs for full behavior.\n\
\n\
Peers:\n\
  /path or c:\\path                 Local path (same as file://)\n\
  sftp://user@host/path            Remote over SSH\n\
  sftp://user@host:port/path       Non-standard SSH port\n\
  sftp://host/path                 Remote over SSH, current OS user\n\
  sftp://user:password@host/path   Inline password (prefer SSH keys)\n\
\n\
Prefix modifiers:\n\
  +<peer>                          Canon - this peer's state wins all conflicts\n\
  -<peer>                          Subordinate - overwritten to match the group\n\
\n\
Fallback URLs (multiple paths to the same data):\n\
  [url1,url2,...]                  Try in order, first that connects wins\n\
  +[url1,url2,...]                 Canon peer with fallbacks\n\
  -[url1,url2,...]                 Subordinate peer with fallbacks\n\
\n\
Per-URL settings (query string, inside quotes):\n\
  \"sftp://host/path?timeout-conn=60\"     Connection timeout for this URL\n\
  \"sftp://host/path?timeout-idle=10\"     SFTP idle keep-alive TTL for this URL\n\
  \"sftp://host/path?timeout-conn=60&timeout-idle=10\"  Combine multiple\n\
\n\
Options:\n\
  --dry-run          Read-only and plan, but make no peer changes\n\
  --max-copies N     Max active file copies across the whole run (default: 10)\n\
  --retries-copy N   Give up copying after this many tries (default: 3)\n\
  --retries-list N   Give up listing after this many tries (default: 3)\n\
  --timeout-conn N   SSH handshake timeout in seconds (default: 30)\n\
  --timeout-idle N   SFTP idle keep-alive TTL in seconds (default: 30)\n\
  --verbosity LEVEL  Verbosity: error, info, debug, trace (default: info)\n\
  -x RELPATH         Exclude relative slash path from sync; repeatable\n\
  --keep-tmp-days N  Delete stale TMP staging after N days (default: 2)\n\
  --keep-bak-days N  Delete displaced files (BAK/) after N days (default: 90)\n\
  --keep-del-days N  Forget deletion records after N days (default: 180)\n\
\n\
Quick start:\n\
  kitchensync +c:/photos sftp://user@host/photos      First sync (c: is canon)\n\
  kitchensync c:/photos sftp://host/photos            Bidirectional\n\
  kitchensync c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate\n\
  kitchensync c:/photos \"sftp://user:p%40ss@host/photos\"  Inline password\n\
\n\
Canon (+) is required on first sync when no peer has snapshot history.\n\
After the first sync, bidirectional sync works without canon.\n\
\n\
Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.\n\
\n\
Displaced files are recoverable from nearby:\n\
  .kitchensync/BAK/ directories (kept for --keep-bak-days days).\n";

struct CommandLineImpl;

impl CommandLine for CommandLineImpl {
    fn parse(&self, args: Vec<String>) -> CommandLineParseResult {
        if args.is_empty() {
            return CommandLineParseResult::Help;
        }

        match parse_run(args) {
            Ok(request) => CommandLineParseResult::Run(request),
            Err(message) => {
                CommandLineParseResult::ValidationError(CommandLineValidationError { message })
            }
        }
    }

    fn help_output(&self) -> CommandLineProcessOutput {
        CommandLineProcessOutput {
            stdout: HELP_TEXT.to_string(),
            exit_code: 0,
        }
    }

    fn validation_error_output(
        &self,
        error: &CommandLineValidationError,
    ) -> CommandLineProcessOutput {
        CommandLineProcessOutput {
            stdout: format!("{}\n\n{}", error.message, HELP_TEXT),
            exit_code: 1,
        }
    }

    fn sync_complete_output(&self) -> CommandLineProcessOutput {
        CommandLineProcessOutput {
            stdout: "sync complete\n".to_string(),
            exit_code: 0,
        }
    }

    fn should_emit(
        &self,
        configured_verbosity: CommandLineVerbosity,
        message_verbosity: CommandLineVerbosity,
    ) -> bool {
        verbosity_rank(message_verbosity) <= verbosity_rank(configured_verbosity)
    }
}

pub fn new() -> std::sync::Arc<dyn CommandLine> {
    Arc::new(CommandLineImpl)
}

fn parse_run(args: Vec<String>) -> Result<CommandLineRunRequest, String> {
    let mut settings = CommandLineSettings {
        dry_run: false,
        max_copies: 10,
        retries_copy: 3,
        retries_list: 3,
        timeout_conn_seconds: 30,
        timeout_idle_seconds: 30,
        verbosity: CommandLineVerbosity::Info,
        excludes: Vec::new(),
        keep_tmp_days: 2,
        keep_bak_days: 90,
        keep_del_days: 180,
    };
    let mut peers = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--dry-run" => {
                settings.dry_run = true;
                index += 1;
            }
            "--max-copies" => {
                settings.max_copies = option_positive_integer(&args, index, "--max-copies")?;
                index += 2;
            }
            "--retries-copy" => {
                settings.retries_copy = option_positive_integer(&args, index, "--retries-copy")?;
                index += 2;
            }
            "--retries-list" => {
                settings.retries_list = option_positive_integer(&args, index, "--retries-list")?;
                index += 2;
            }
            "--timeout-conn" => {
                settings.timeout_conn_seconds =
                    option_positive_integer(&args, index, "--timeout-conn")?;
                index += 2;
            }
            "--timeout-idle" => {
                settings.timeout_idle_seconds =
                    option_positive_integer(&args, index, "--timeout-idle")?;
                index += 2;
            }
            "--verbosity" => {
                let value = option_value(&args, index, "--verbosity")?;
                settings.verbosity = parse_verbosity(value)?;
                index += 2;
            }
            "-x" => {
                let value = option_value(&args, index, "-x")?;
                validate_exclude(value)?;
                settings.excludes.push(value.to_string());
                index += 2;
            }
            "--keep-tmp-days" => {
                settings.keep_tmp_days =
                    option_positive_integer(&args, index, "--keep-tmp-days")?;
                index += 2;
            }
            "--keep-bak-days" => {
                settings.keep_bak_days =
                    option_positive_integer(&args, index, "--keep-bak-days")?;
                index += 2;
            }
            "--keep-del-days" => {
                settings.keep_del_days =
                    option_positive_integer(&args, index, "--keep-del-days")?;
                index += 2;
            }
            _ if arg.starts_with("--") => {
                return Err(format!("unrecognized option {}", arg));
            }
            _ => {
                peers.push(parse_peer(arg)?);
                index += 1;
            }
        }
    }

    if peers.len() < 2 {
        return Err("at least two peers are required".to_string());
    }

    let canon_count = peers
        .iter()
        .filter(|peer| peer.role == CommandLinePeerRole::Canon)
        .count();
    if canon_count > 1 {
        return Err("at most one canon peer is allowed".to_string());
    }

    Ok(CommandLineRunRequest { settings, peers })
}

fn option_value<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str, String> {
    args.get(index + 1)
        .map(|value| value.as_str())
        .ok_or_else(|| format!("{} requires a value", name))
}

fn option_positive_integer(args: &[String], index: usize, name: &str) -> Result<u64, String> {
    parse_positive_integer(option_value(args, index, name)?)
        .ok_or_else(|| format!("{} requires a positive integer", name))
}

fn parse_positive_integer(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    match value.parse::<u64>() {
        Ok(number) if number > 0 => Some(number),
        _ => None,
    }
}

fn parse_verbosity(value: &str) -> Result<CommandLineVerbosity, String> {
    match value {
        "error" => Ok(CommandLineVerbosity::Error),
        "info" => Ok(CommandLineVerbosity::Info),
        "debug" => Ok(CommandLineVerbosity::Debug),
        "trace" => Ok(CommandLineVerbosity::Trace),
        _ => Err("--verbosity requires error, info, debug, or trace".to_string()),
    }
}

fn validate_exclude(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains('\\')
        || value.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("-x requires a relative slash path".to_string());
    }
    Ok(())
}

fn parse_peer(arg: &str) -> Result<CommandLinePeer, String> {
    let (role, value) = match arg.as_bytes().first() {
        Some(b'+') => (CommandLinePeerRole::Canon, &arg[1..]),
        Some(b'-') => (CommandLinePeerRole::Subordinate, &arg[1..]),
        _ => (CommandLinePeerRole::Normal, arg),
    };

    if value.is_empty() {
        return Err("peer cannot be empty".to_string());
    }

    let url_values = if value.starts_with('[') || value.ends_with(']') {
        if !(value.starts_with('[') && value.ends_with(']')) {
            return Err("fallback peer must use matching brackets".to_string());
        }
        let inner = &value[1..value.len() - 1];
        if inner.is_empty() {
            return Err("fallback peer must contain at least one URL".to_string());
        }
        inner.split(',').collect::<Vec<_>>()
    } else {
        vec![value]
    };

    let mut urls = Vec::with_capacity(url_values.len());
    for url_value in url_values {
        if url_value.is_empty() {
            return Err("fallback URL cannot be empty".to_string());
        }
        urls.push(parse_url_alternative(url_value)?);
    }

    Ok(CommandLinePeer { role, urls })
}

fn parse_url_alternative(value: &str) -> Result<CommandLineUrlAlternative, String> {
    let mut timeout_conn_seconds = None;
    let mut timeout_idle_seconds = None;

    if let Some(query) = value.split_once('?').map(|(_, query)| query) {
        for pair in query.split('&') {
            let (name, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
            let parsed_value = parse_positive_integer(raw_value).ok_or_else(|| {
                format!("URL setting {} requires a positive integer", name)
            })?;
            match name {
                "timeout-conn" => timeout_conn_seconds = Some(parsed_value),
                "timeout-idle" => timeout_idle_seconds = Some(parsed_value),
                _ => return Err(format!("unrecognized URL setting {}", name)),
            }
        }
    }

    let url = if has_scheme(value) {
        validate_schemed_url(value)?;
        value.to_string()
    } else {
        format!("file://{}", value)
    };

    Ok(CommandLineUrlAlternative {
        url,
        timeout_conn_seconds,
        timeout_idle_seconds,
    })
}

fn has_scheme(value: &str) -> bool {
    value.contains("://")
}

fn validate_schemed_url(value: &str) -> Result<(), String> {
    if value.starts_with("sftp://") {
        validate_sftp_url(value)
    } else if value.starts_with("file://") {
        Ok(())
    } else {
        Err("peer URL must be a local path or sftp URL".to_string())
    }
}

fn validate_sftp_url(value: &str) -> Result<(), String> {
    let without_scheme = &value["sftp://".len()..];
    let before_query = without_scheme.split_once('?').map_or(without_scheme, |(head, _)| head);
    let slash_index = before_query
        .find('/')
        .ok_or_else(|| "sftp URL requires an absolute path".to_string())?;
    if slash_index == 0 {
        return Err("sftp URL requires a host".to_string());
    }

    let authority = &before_query[..slash_index];
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    if host_port.is_empty() || host_port.starts_with(':') {
        return Err("sftp URL requires a host".to_string());
    }
    if host_port.ends_with(':') {
        return Err("sftp URL port cannot be empty".to_string());
    }
    if let Some((_, port)) = host_port.rsplit_once(':') {
        if !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("sftp URL port must be numeric".to_string());
        }
    }

    Ok(())
}

fn verbosity_rank(verbosity: CommandLineVerbosity) -> u8 {
    match verbosity {
        CommandLineVerbosity::Error => 0,
        CommandLineVerbosity::Info | CommandLineVerbosity::Debug => 1,
        CommandLineVerbosity::Trace => 3,
    }
}
