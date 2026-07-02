use crate::api::*;
use percent_encoding::percent_decode_str;
use std::sync::Arc;
use url::Url;

struct PeerArgumentParserImpl;

impl PeerArgumentParser for PeerArgumentParserImpl {
    fn parse_peer_arguments(
        &self,
        peer_operands: Vec<String>,
        global_timeout_conn_seconds: u32,
        global_timeout_idle_seconds: u32,
        current_os_username: String,
    ) -> PeerArgumentParseResult {
        if peer_operands.len() < 2 {
            return PeerArgumentParseResult::ValidationFailure(
                PeerArgumentValidationReason::TooFewPeerOperands,
            );
        }

        let mut peers = Vec::new();
        let mut canon_count = 0;

        for operand in peer_operands {
            let (role, target_text) = split_role(&operand);
            if role == PeerArgumentPeerRole::Canon {
                canon_count += 1;
                if canon_count > 1 {
                    return PeerArgumentParseResult::ValidationFailure(
                        PeerArgumentValidationReason::MoreThanOneCanonPeer,
                    );
                }
            }

            let members = fallback_members(target_text);
            let mut fallback_targets = Vec::new();
            for member in members {
                match parse_target(
                    member,
                    global_timeout_conn_seconds,
                    global_timeout_idle_seconds,
                    &current_os_username,
                ) {
                    Ok(target) => fallback_targets.push(target),
                    Err(reason) => return PeerArgumentParseResult::ValidationFailure(reason),
                }
            }

            peers.push(PeerArgumentPeer {
                role,
                fallback_targets,
            });
        }

        PeerArgumentParseResult::Parsed(peers)
    }
}

pub fn new() -> std::sync::Arc<dyn PeerArgumentParser> {
    Arc::new(PeerArgumentParserImpl)
}

fn split_role(operand: &str) -> (PeerArgumentPeerRole, &str) {
    if let Some(rest) = operand.strip_prefix('+') {
        (PeerArgumentPeerRole::Canon, rest)
    } else if let Some(rest) = operand.strip_prefix('-') {
        (PeerArgumentPeerRole::Subordinate, rest)
    } else {
        (PeerArgumentPeerRole::Normal, operand)
    }
}

fn fallback_members(target_text: &str) -> Vec<&str> {
    if target_text.starts_with('[') && target_text.ends_with(']') {
        target_text[1..target_text.len() - 1].split(',').collect()
    } else {
        vec![target_text]
    }
}

fn parse_target(
    target_text: &str,
    global_timeout_conn_seconds: u32,
    global_timeout_idle_seconds: u32,
    current_os_username: &str,
) -> Result<PeerArgumentTarget, PeerArgumentValidationReason> {
    if !has_url_scheme(target_text) {
        return Ok(local_target(
            target_text.to_owned(),
            global_timeout_conn_seconds,
            global_timeout_idle_seconds,
        ));
    }

    let url =
        Url::parse(target_text).map_err(|_| PeerArgumentValidationReason::UnsupportedPeerUrlForm)?;
    let connection = parse_query_settings(
        &url,
        global_timeout_conn_seconds,
        global_timeout_idle_seconds,
    )?;

    match url.scheme() {
        "file" => Ok(PeerArgumentTarget {
            location: PeerArgumentLocation::Local(PeerArgumentLocalTarget {
                path_or_url: target_text.to_owned(),
            }),
            connection,
        }),
        "sftp" => parse_sftp_target(&url, connection, current_os_username),
        _ => Err(PeerArgumentValidationReason::UnsupportedPeerUrlForm),
    }
}

fn local_target(
    path_or_url: String,
    timeout_conn_seconds: u32,
    timeout_idle_seconds: u32,
) -> PeerArgumentTarget {
    PeerArgumentTarget {
        location: PeerArgumentLocation::Local(PeerArgumentLocalTarget { path_or_url }),
        connection: PeerArgumentUrlConnectionSettings {
            timeout_conn_seconds,
            timeout_idle_seconds,
        },
    }
}

fn parse_sftp_target(
    url: &Url,
    connection: PeerArgumentUrlConnectionSettings,
    current_os_username: &str,
) -> Result<PeerArgumentTarget, PeerArgumentValidationReason> {
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or(PeerArgumentValidationReason::UnsupportedPeerUrlForm)?;
    let absolute_path = url.path();
    if !absolute_path.starts_with('/') {
        return Err(PeerArgumentValidationReason::UnsupportedPeerUrlForm);
    }

    let username = if url.username().is_empty() {
        current_os_username.to_owned()
    } else {
        url.username().to_owned()
    };
    let password = match url.password() {
        Some(password) => Some(
            percent_decode_str(password)
                .decode_utf8()
                .map_err(|_| PeerArgumentValidationReason::UnsupportedPeerUrlForm)?
                .into_owned(),
        ),
        None => None,
    };

    Ok(PeerArgumentTarget {
        location: PeerArgumentLocation::Sftp(PeerArgumentSftpTarget {
            host: host.to_owned(),
            username,
            password,
            port: url.port().unwrap_or(22),
            absolute_path: absolute_path.to_owned(),
        }),
        connection,
    })
}

fn parse_query_settings(
    url: &Url,
    global_timeout_conn_seconds: u32,
    global_timeout_idle_seconds: u32,
) -> Result<PeerArgumentUrlConnectionSettings, PeerArgumentValidationReason> {
    let mut settings = PeerArgumentUrlConnectionSettings {
        timeout_conn_seconds: global_timeout_conn_seconds,
        timeout_idle_seconds: global_timeout_idle_seconds,
    };

    for (name, value) in url.query_pairs() {
        match name.as_ref() {
            "timeout-conn" => settings.timeout_conn_seconds = parse_timeout_value(&value)?,
            "timeout-idle" => settings.timeout_idle_seconds = parse_timeout_value(&value)?,
            _ => return Err(PeerArgumentValidationReason::UnsupportedQueryParameter),
        }
    }

    Ok(settings)
}

fn parse_timeout_value(value: &str) -> Result<u32, PeerArgumentValidationReason> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(PeerArgumentValidationReason::InvalidUrlTimeoutValue);
    }

    match value.parse::<u32>() {
        Ok(number) if number > 0 => Ok(number),
        _ => Err(PeerArgumentValidationReason::InvalidUrlTimeoutValue),
    }
}

fn has_url_scheme(target_text: &str) -> bool {
    let Some(colon_index) = target_text.find(':') else {
        return false;
    };

    if is_windows_drive_path(target_text) {
        return false;
    }

    let scheme = &target_text[..colon_index];
    let mut chars = scheme.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.')
}

fn is_windows_drive_path(target_text: &str) -> bool {
    let bytes = target_text.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}
