use crate::api::*;
use std::sync::Arc;

const HELP_SPEC: &str = include_str!("../../../../specs/help.md");

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
            stdout: help_text().to_string(),
            exit_code: 0,
        }
    }

    fn validation_error_output(
        &self,
        error: &CommandLineValidationError,
    ) -> CommandLineProcessOutput {
        CommandLineProcessOutput {
            stdout: format!("{}\n\n{}", error.message, help_text()),
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
        match configured_verbosity {
            CommandLineVerbosity::Error => message_verbosity == CommandLineVerbosity::Error,
            CommandLineVerbosity::Info | CommandLineVerbosity::Debug => {
                matches!(
                    message_verbosity,
                    CommandLineVerbosity::Error | CommandLineVerbosity::Info
                )
            }
            CommandLineVerbosity::Trace => true,
        }
    }
}

pub fn new() -> std::sync::Arc<dyn CommandLine> {
    Arc::new(CommandLineImpl)
}

fn help_text() -> &'static str {
    HELP_SPEC
        .split_once("```\n")
        .and_then(|(_, rest)| rest.split_once("```"))
        .map(|(screen, _)| screen)
        .expect("help.md must contain the verbatim help screen")
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
