use std::sync::Arc;
use crate::api::*;

struct GlobalArgumentParserImpl;

impl GlobalArgumentParser for GlobalArgumentParserImpl {
    fn parse_global_arguments(
        &self,
        args: Vec<String>,
        help_text: String,
    ) -> GlobalArgumentParseResult {
        if args.is_empty() {
            return GlobalArgumentParseResult::Help(GlobalCommandOutput {
                stdout: help_text,
                stderr: String::new(),
                exit_code: 0,
            });
        }

        let mut settings = GlobalRunSettings {
            dry_run: false,
            max_copies: 10,
            retries_copy: 3,
            retries_list: 3,
            timeout_conn_seconds: 30,
            timeout_idle_seconds: 30,
            verbosity: GlobalVerbosity::Info,
            keep_tmp_days: 2,
            keep_bak_days: 90,
            keep_del_days: 180,
            excludes: Vec::new(),
        };

        let mut peer_operands = Vec::new();
        let mut index = 0;

        while index < args.len() {
            let arg = &args[index];

            match arg.as_str() {
                "--dry-run" => {
                    settings.dry_run = true;
                    index += 1;
                }
                "--max-copies" => {
                    let value = match option_value(&args, index, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    settings.max_copies = match parse_positive_integer(value, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    index += 2;
                }
                "--retries-copy" => {
                    let value = match option_value(&args, index, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    settings.retries_copy = match parse_positive_integer(value, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    index += 2;
                }
                "--retries-list" => {
                    let value = match option_value(&args, index, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    settings.retries_list = match parse_positive_integer(value, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    index += 2;
                }
                "--timeout-conn" => {
                    let value = match option_value(&args, index, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    settings.timeout_conn_seconds =
                        match parse_positive_integer(value, arg, &help_text) {
                            Ok(value) => value,
                            Err(result) => return result,
                        };
                    index += 2;
                }
                "--timeout-idle" => {
                    let value = match option_value(&args, index, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    settings.timeout_idle_seconds =
                        match parse_positive_integer(value, arg, &help_text) {
                            Ok(value) => value,
                            Err(result) => return result,
                        };
                    index += 2;
                }
                "--keep-tmp-days" => {
                    let value = match option_value(&args, index, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    settings.keep_tmp_days = match parse_positive_integer(value, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    index += 2;
                }
                "--keep-bak-days" => {
                    let value = match option_value(&args, index, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    settings.keep_bak_days = match parse_positive_integer(value, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    index += 2;
                }
                "--keep-del-days" => {
                    let value = match option_value(&args, index, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    settings.keep_del_days = match parse_positive_integer(value, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    index += 2;
                }
                "--verbosity" => {
                    let value = match option_value(&args, index, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    settings.verbosity = match parse_verbosity(value, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    index += 2;
                }
                "-x" => {
                    let value = match option_value(&args, index, arg, &help_text) {
                        Ok(value) => value,
                        Err(result) => return result,
                    };
                    if !is_valid_relative_slash_path(value) {
                        return validation_failure(
                            format!("Invalid exclude path for -x: {value}"),
                            help_text,
                        );
                    }
                    settings.excludes.push(value.clone());
                    index += 2;
                }
                _ if arg.starts_with('-') => {
                    return validation_failure(
                        format!("Unrecognized global option: {arg}"),
                        help_text,
                    );
                }
                _ => {
                    peer_operands.extend(args[index..].iter().cloned());
                    break;
                }
            }
        }

        GlobalArgumentParseResult::Run(GlobalArgumentRunRequest {
            settings,
            peer_operands,
        })
    }
}

pub fn new() -> std::sync::Arc<dyn GlobalArgumentParser> {
    Arc::new(GlobalArgumentParserImpl)
}

fn option_value<'a>(
    args: &'a [String],
    index: usize,
    option_name: &str,
    help_text: &str,
) -> Result<&'a String, GlobalArgumentParseResult> {
    args.get(index + 1).ok_or_else(|| {
        validation_failure(
            format!("Missing value for global option: {option_name}"),
            help_text.to_owned(),
        )
    })
}

fn parse_positive_integer(
    value: &str,
    option_name: &str,
    help_text: &str,
) -> Result<u32, GlobalArgumentParseResult> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(validation_failure(
            format!("Invalid positive integer for {option_name}: {value}"),
            help_text.to_owned(),
        ));
    }

    match value.parse::<u32>() {
        Ok(number) if number > 0 => Ok(number),
        _ => Err(validation_failure(
            format!("Invalid positive integer for {option_name}: {value}"),
            help_text.to_owned(),
        )),
    }
}

fn parse_verbosity(
    value: &str,
    help_text: &str,
) -> Result<GlobalVerbosity, GlobalArgumentParseResult> {
    match value {
        "error" => Ok(GlobalVerbosity::Error),
        "info" => Ok(GlobalVerbosity::Info),
        "debug" => Ok(GlobalVerbosity::Debug),
        "trace" => Ok(GlobalVerbosity::Trace),
        _ => Err(validation_failure(
            format!("Invalid verbosity level: {value}"),
            help_text.to_owned(),
        )),
    }
}

fn is_valid_relative_slash_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains('\\')
        && !value.contains('\0')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn validation_failure(message: String, help_text: String) -> GlobalArgumentParseResult {
    GlobalArgumentParseResult::ValidationFailure(GlobalCommandOutput {
        stdout: format!("{message}\n{help_text}"),
        stderr: String::new(),
        exit_code: 1,
    })
}
