use crate::api::*;
use std::path::PathBuf;
use std::sync::Arc;
use url::Url;

const HELP_TEXT: &str = include_str!("../../../../specs/help.md");

struct CommandAndOutputImpl {
    globalargumentparser: std::sync::Arc<dyn commandandoutput_globalargumentparser::GlobalArgumentParser>,
    peerargumentparser: std::sync::Arc<dyn commandandoutput_peerargumentparser::PeerArgumentParser>,
    peeridentitynormalizer: std::sync::Arc<dyn commandandoutput_peeridentitynormalizer::PeerIdentityNormalizer>,
    stdoutreporter: std::sync::Arc<dyn commandandoutput_stdoutreporter::StdoutReporter>,
}

impl CommandAndOutput for CommandAndOutputImpl {
    fn parse_command(
        &self,
        args: Vec<String>,
        current_working_directory: PathBuf,
        current_os_username: String,
    ) -> CommandParseResult {
        let help_text = fenced_help_text();
        let global_result = self
            .globalargumentparser
            .parse_global_arguments(args, help_text.clone());

        let global_run = match global_result {
            commandandoutput_globalargumentparser::GlobalArgumentParseResult::Help(output) => {
                return CommandParseResult::Help(command_output(output));
            }
            commandandoutput_globalargumentparser::GlobalArgumentParseResult::ValidationFailure(
                output,
            ) => {
                return CommandParseResult::ValidationFailure(command_output(output));
            }
            commandandoutput_globalargumentparser::GlobalArgumentParseResult::Run(run) => run,
        };

        let explicit_usernames = sftp_username_explicitness(&global_run.peer_operands);
        let peer_result = self.peerargumentparser.parse_peer_arguments(
            global_run.peer_operands,
            global_run.settings.timeout_conn_seconds,
            global_run.settings.timeout_idle_seconds,
            current_os_username.clone(),
        );

        let parsed_peers = match peer_result {
            commandandoutput_peerargumentparser::PeerArgumentParseResult::Parsed(peers) => peers,
            commandandoutput_peerargumentparser::PeerArgumentParseResult::ValidationFailure(
                reason,
            ) => {
                return CommandParseResult::ValidationFailure(validation_output(
                    peer_validation_message(reason),
                    help_text,
                ));
            }
        };

        let mut explicit_index = 0;
        let mut peers = Vec::new();
        for parsed_peer in parsed_peers {
            let mut fallback_targets = Vec::new();
            for parsed_target in parsed_peer.fallback_targets {
                let username_was_explicit = explicit_usernames
                    .get(explicit_index)
                    .copied()
                    .unwrap_or(false);
                explicit_index += 1;

                let location = peer_location(parsed_target.location, username_was_explicit);
                let normalized_identity = match self.peeridentitynormalizer.normalize_peer_identity(
                    identity_target(&location),
                    current_working_directory.clone(),
                    current_os_username.clone(),
                ) {
                    Ok(identity) => identity,
                    Err(error) => {
                        return CommandParseResult::ValidationFailure(validation_output(
                            error.message,
                            help_text,
                        ));
                    }
                };

                fallback_targets.push(PeerTarget {
                    location,
                    connection: url_connection_settings(parsed_target.connection),
                    normalized_identity,
                });
            }

            peers.push(Peer {
                role: peer_role(parsed_peer.role),
                fallback_targets,
            });
        }

        CommandParseResult::Run(RunRequest {
            settings: run_settings(global_run.settings),
            peers,
        })
    }

    fn normalize_peer_identity(
        &self,
        target: PeerLocation,
        current_working_directory: PathBuf,
        current_os_username: String,
    ) -> Result<String, PeerIdentityError> {
        self.peeridentitynormalizer
            .normalize_peer_identity(
                identity_target(&target),
                current_working_directory,
                current_os_username,
            )
            .map_err(|error| PeerIdentityError {
                target,
                message: error.message,
            })
    }

    fn write_output(&self, verbosity: Verbosity, event: OutputEvent) {
        let verbosity = stdout_verbosity(verbosity);
        match event {
            OutputEvent::ArgumentValidationFailure(output) => self
                .stdoutreporter
                .report_argument_validation_failure(
                    verbosity,
                    output.error_message,
                    output.help_text,
                ),
            OutputEvent::FirstSyncNeedsCanon => self
                .stdoutreporter
                .report_first_sync_requires_authoritative_peer(verbosity),
            OutputEvent::NoContributingPeerReachable => self
                .stdoutreporter
                .report_no_contributing_peer_reachable(verbosity),
            OutputEvent::ErrorDiagnostic(diagnostic) => self.stdoutreporter.report_error_diagnostic(
                verbosity,
                commandandoutput_stdoutreporter::StdoutErrorDiagnostic {
                    kind: stdout_error_kind(diagnostic.kind),
                    details: diagnostic.details,
                },
            ),
            OutputEvent::FailedFileTransfer(diagnostic) => self
                .stdoutreporter
                .report_failed_file_transfer(
                    verbosity,
                    commandandoutput_stdoutreporter::StdoutFailedFileTransferDiagnostic {
                        relpath: diagnostic.relpath,
                        destination_peer_url: diagnostic.destination_peer_url,
                        phase: stdout_transfer_phase(diagnostic.phase),
                        transport_error_category: diagnostic.transport_error_category,
                    },
                ),
            OutputEvent::CopyProgress { relpath } => {
                self.stdoutreporter.report_copy_progress(verbosity, relpath)
            }
            OutputEvent::DisplacementProgress { relpath } => self
                .stdoutreporter
                .report_displacement_progress(verbosity, relpath),
            OutputEvent::CopySlots { active, max } => {
                self.stdoutreporter
                    .report_copy_slots(verbosity, active, max)
            }
            OutputEvent::Completion => {
                self.stdoutreporter
                    .report_completion(verbosity, "sync complete".to_owned())
            }
        }
    }
}

pub fn new(globalargumentparser: std::sync::Arc<dyn commandandoutput_globalargumentparser::GlobalArgumentParser>, peerargumentparser: std::sync::Arc<dyn commandandoutput_peerargumentparser::PeerArgumentParser>, peeridentitynormalizer: std::sync::Arc<dyn commandandoutput_peeridentitynormalizer::PeerIdentityNormalizer>, stdoutreporter: std::sync::Arc<dyn commandandoutput_stdoutreporter::StdoutReporter>) -> std::sync::Arc<dyn CommandAndOutput> {
    Arc::new(CommandAndOutputImpl { globalargumentparser, peerargumentparser, peeridentitynormalizer, stdoutreporter })
}

fn fenced_help_text() -> String {
    let start = HELP_TEXT
        .find("```\n")
        .expect("help fence starts")
        + 4;
    let end = HELP_TEXT[start..]
        .find("\n```")
        .expect("help fence ends")
        + start;
    HELP_TEXT[start..end].to_owned()
}

fn command_output(
    output: commandandoutput_globalargumentparser::GlobalCommandOutput,
) -> CommandProcessOutput {
    CommandProcessOutput {
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: output.exit_code,
    }
}

fn validation_output(error_message: String, help_text: String) -> CommandProcessOutput {
    CommandProcessOutput {
        stdout: format!("{error_message}\n{help_text}"),
        stderr: String::new(),
        exit_code: 1,
    }
}

fn peer_validation_message(
    reason: commandandoutput_peerargumentparser::PeerArgumentValidationReason,
) -> String {
    match reason {
        commandandoutput_peerargumentparser::PeerArgumentValidationReason::TooFewPeerOperands => {
            "at least two peer operands are required"
        }
        commandandoutput_peerargumentparser::PeerArgumentValidationReason::MoreThanOneCanonPeer => {
            "at most one canon peer is allowed"
        }
        commandandoutput_peerargumentparser::PeerArgumentValidationReason::UnsupportedPeerUrlForm => {
            "unsupported peer URL form"
        }
        commandandoutput_peerargumentparser::PeerArgumentValidationReason::UnsupportedQueryParameter => {
            "unsupported URL query parameter"
        }
        commandandoutput_peerargumentparser::PeerArgumentValidationReason::InvalidUrlTimeoutValue => {
            "URL timeout values must be positive integers"
        }
    }
    .to_owned()
}

fn run_settings(
    settings: commandandoutput_globalargumentparser::GlobalRunSettings,
) -> RunSettings {
    RunSettings {
        dry_run: settings.dry_run,
        max_copies: settings.max_copies,
        retries_copy: settings.retries_copy,
        retries_list: settings.retries_list,
        timeout_conn_seconds: settings.timeout_conn_seconds,
        timeout_idle_seconds: settings.timeout_idle_seconds,
        verbosity: verbosity(settings.verbosity),
        keep_tmp_days: settings.keep_tmp_days,
        keep_bak_days: settings.keep_bak_days,
        keep_del_days: settings.keep_del_days,
        excludes: settings.excludes,
    }
}

fn verbosity(value: commandandoutput_globalargumentparser::GlobalVerbosity) -> Verbosity {
    match value {
        commandandoutput_globalargumentparser::GlobalVerbosity::Error => Verbosity::Error,
        commandandoutput_globalargumentparser::GlobalVerbosity::Info => Verbosity::Info,
        commandandoutput_globalargumentparser::GlobalVerbosity::Debug => Verbosity::Debug,
        commandandoutput_globalargumentparser::GlobalVerbosity::Trace => Verbosity::Trace,
    }
}

fn peer_role(role: commandandoutput_peerargumentparser::PeerArgumentPeerRole) -> PeerRole {
    match role {
        commandandoutput_peerargumentparser::PeerArgumentPeerRole::Canon => PeerRole::Canon,
        commandandoutput_peerargumentparser::PeerArgumentPeerRole::Subordinate => {
            PeerRole::Subordinate
        }
        commandandoutput_peerargumentparser::PeerArgumentPeerRole::Normal => PeerRole::Normal,
    }
}

fn peer_location(
    location: commandandoutput_peerargumentparser::PeerArgumentLocation,
    username_was_explicit: bool,
) -> PeerLocation {
    match location {
        commandandoutput_peerargumentparser::PeerArgumentLocation::Local(local) => {
            PeerLocation::Local(LocalPeerTarget {
                path_or_url: local.path_or_url,
            })
        }
        commandandoutput_peerargumentparser::PeerArgumentLocation::Sftp(sftp) => {
            PeerLocation::Sftp(SftpPeerTarget {
                host: sftp.host,
                username: sftp.username,
                username_was_explicit,
                password: sftp.password,
                port: sftp.port,
                absolute_path: sftp.absolute_path,
            })
        }
    }
}

fn url_connection_settings(
    settings: commandandoutput_peerargumentparser::PeerArgumentUrlConnectionSettings,
) -> UrlConnectionSettings {
    UrlConnectionSettings {
        timeout_conn_seconds: settings.timeout_conn_seconds,
        timeout_idle_seconds: settings.timeout_idle_seconds,
    }
}

fn identity_target(
    target: &PeerLocation,
) -> commandandoutput_peeridentitynormalizer::PeerIdentityTarget {
    match target {
        PeerLocation::Local(local) => {
            commandandoutput_peeridentitynormalizer::PeerIdentityTarget::Local(
                commandandoutput_peeridentitynormalizer::LocalPeerIdentityTarget {
                    path_or_url: local.path_or_url.clone(),
                },
            )
        }
        PeerLocation::Sftp(sftp) => {
            commandandoutput_peeridentitynormalizer::PeerIdentityTarget::Sftp(
                commandandoutput_peeridentitynormalizer::SftpPeerIdentityTarget {
                    host: sftp.host.clone(),
                    username: if sftp.username_was_explicit {
                        Some(sftp.username.clone())
                    } else {
                        None
                    },
                    port: sftp.port,
                    absolute_path: sftp.absolute_path.clone(),
                },
            )
        }
    }
}

fn sftp_username_explicitness(peer_operands: &[String]) -> Vec<bool> {
    let mut values = Vec::new();
    for operand in peer_operands {
        let target_text = operand
            .strip_prefix('+')
            .or_else(|| operand.strip_prefix('-'))
            .unwrap_or(operand);
        for member in fallback_members(target_text) {
            values.push(sftp_username_was_explicit(member));
        }
    }
    values
}

fn fallback_members(target_text: &str) -> Vec<&str> {
    if target_text.starts_with('[') && target_text.ends_with(']') {
        target_text[1..target_text.len() - 1].split(',').collect()
    } else {
        vec![target_text]
    }
}

fn sftp_username_was_explicit(target_text: &str) -> bool {
    Url::parse(target_text)
        .ok()
        .filter(|url| url.scheme() == "sftp")
        .is_some_and(|url| !url.username().is_empty())
}

fn stdout_verbosity(verbosity: Verbosity) -> commandandoutput_stdoutreporter::StdoutVerbosity {
    match verbosity {
        Verbosity::Error => commandandoutput_stdoutreporter::StdoutVerbosity::Error,
        Verbosity::Info => commandandoutput_stdoutreporter::StdoutVerbosity::Info,
        Verbosity::Debug => commandandoutput_stdoutreporter::StdoutVerbosity::Debug,
        Verbosity::Trace => commandandoutput_stdoutreporter::StdoutVerbosity::Trace,
    }
}

fn stdout_error_kind(kind: SyncErrorKind) -> commandandoutput_stdoutreporter::StdoutErrorKind {
    match kind {
        SyncErrorKind::NoSnapshotsAndNoCanon => commandandoutput_stdoutreporter::StdoutErrorKind::NoSnapshotsAndNoCanon,
        SyncErrorKind::UnreachablePeer => commandandoutput_stdoutreporter::StdoutErrorKind::UnreachablePeer,
        SyncErrorKind::DirectoryListingFailure => commandandoutput_stdoutreporter::StdoutErrorKind::DirectoryListingFailure,
        SyncErrorKind::CanonPeerUnreachable => commandandoutput_stdoutreporter::StdoutErrorKind::CanonPeerUnreachable,
        SyncErrorKind::FewerThanTwoReachablePeers => commandandoutput_stdoutreporter::StdoutErrorKind::FewerThanTwoReachablePeers,
        SyncErrorKind::NoContributingPeerReachable => commandandoutput_stdoutreporter::StdoutErrorKind::NoContributingPeerReachable,
        SyncErrorKind::TransferFailureBeforeSwapOld => commandandoutput_stdoutreporter::StdoutErrorKind::TransferFailureBeforeSwapOld,
        SyncErrorKind::TransferFailureAfterSwapOld => commandandoutput_stdoutreporter::StdoutErrorKind::TransferFailureAfterSwapOld,
        SyncErrorKind::ArchiveOldFailure => commandandoutput_stdoutreporter::StdoutErrorKind::ArchiveOldFailure,
        SyncErrorKind::DisplacementFailure => commandandoutput_stdoutreporter::StdoutErrorKind::DisplacementFailure,
        SyncErrorKind::TmpOrSwapStagingFailure => commandandoutput_stdoutreporter::StdoutErrorKind::TmpOrSwapStagingFailure,
        SyncErrorKind::SetModTimeFailure => commandandoutput_stdoutreporter::StdoutErrorKind::SetModTimeFailure,
        SyncErrorKind::SnapshotUploadFailureBeforeSwapOld => commandandoutput_stdoutreporter::StdoutErrorKind::SnapshotUploadFailureBeforeSwapOld,
        SyncErrorKind::SnapshotUploadFailureAfterSwapOld => commandandoutput_stdoutreporter::StdoutErrorKind::SnapshotUploadFailureAfterSwapOld,
    }
}

fn stdout_transfer_phase(
    phase: FileTransferPhase,
) -> commandandoutput_stdoutreporter::StdoutFileTransferPhase {
    match phase {
        FileTransferPhase::ReadSource => commandandoutput_stdoutreporter::StdoutFileTransferPhase::ReadSource,
        FileTransferPhase::WriteSwapNew => commandandoutput_stdoutreporter::StdoutFileTransferPhase::WriteSwapNew,
        FileTransferPhase::MoveExistingToSwapOld => commandandoutput_stdoutreporter::StdoutFileTransferPhase::MoveExistingToSwapOld,
        FileTransferPhase::RenameFinal => commandandoutput_stdoutreporter::StdoutFileTransferPhase::RenameFinal,
        FileTransferPhase::SetModTime => commandandoutput_stdoutreporter::StdoutFileTransferPhase::SetModTime,
        FileTransferPhase::ArchiveOld => commandandoutput_stdoutreporter::StdoutFileTransferPhase::ArchiveOld,
        FileTransferPhase::Cleanup => commandandoutput_stdoutreporter::StdoutFileTransferPhase::Cleanup,
    }
}
