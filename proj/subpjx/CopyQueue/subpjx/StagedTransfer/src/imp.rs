use std::io::{Read, Write};
use std::sync::Arc;

use crate::api::*;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS.add(b'%').add(b'/').add(b'\\');
const COPY_BUFFER_SIZE: usize = 64 * 1024;

struct StagedTransferImpl;

impl StagedTransfer for StagedTransferImpl {
    fn run_transfer_try(
        &self,
        request: StagedTransferRequest,
        file_operations: &dyn StagedTransferFileOperations,
        swap_recovery: &dyn StagedTransferSwapRecovery,
        timestamp_generator: &dyn StagedTransferTimestampGenerator,
    ) -> StagedTransferTryOutcome {
        let destination = DestinationPaths::new(&request.relative_destination_file_path);

        if let Err(error) = swap_recovery.recover_user_data_swap(
            &request.destination_peer,
            &destination.target_parent,
            &destination.basename,
            &destination.encoded_basename,
        ) {
            return StagedTransferTryOutcome::RecoveryFailure(error);
        }

        if let Err(error) =
            file_operations.create_directory_all(&request.destination_peer, &destination.swap_dir)
        {
            return StagedTransferTryOutcome::Failure(failure(
                StagedTransferFailurePhase::WriteSwapNew,
                StagedTransferSwapOldState::NotCreated,
                error,
            ));
        }

        if let Err((phase, error)) = stream_to_swap_new(&request, file_operations, &destination) {
            cleanup_swap_new(file_operations, &request.destination_peer, &destination.swap_new);
            return StagedTransferTryOutcome::Failure(failure(
                phase,
                StagedTransferSwapOldState::NotCreated,
                error,
            ));
        }

        let target_existed = match file_operations
            .file_exists(&request.destination_peer, &request.relative_destination_file_path)
        {
            Ok(exists) => exists,
            Err(error) => {
                cleanup_swap_new(file_operations, &request.destination_peer, &destination.swap_new);
                return StagedTransferTryOutcome::Failure(failure(
                    StagedTransferFailurePhase::MoveExistingToSwapOld,
                    StagedTransferSwapOldState::NotCreated,
                    error,
                ));
            }
        };

        if target_existed {
            if let Err(error) = file_operations.rename_to_missing_path(
                &request.destination_peer,
                &request.relative_destination_file_path,
                &destination.swap_old,
            ) {
                cleanup_swap_new(file_operations, &request.destination_peer, &destination.swap_new);
                return StagedTransferTryOutcome::SkipRestOfRun(failure(
                    StagedTransferFailurePhase::MoveExistingToSwapOld,
                    StagedTransferSwapOldState::NotCreated,
                    error,
                ));
            }
        }

        let swap_old_state = if target_existed {
            StagedTransferSwapOldState::Created
        } else {
            StagedTransferSwapOldState::NotCreated
        };

        if let Err(error) = file_operations.rename_to_missing_path(
            &request.destination_peer,
            &destination.swap_new,
            &request.relative_destination_file_path,
        ) {
            if !target_existed {
                cleanup_swap_new(file_operations, &request.destination_peer, &destination.swap_new);
            }
            return StagedTransferTryOutcome::Failure(failure(
                StagedTransferFailurePhase::RenameFinal,
                swap_old_state,
                error,
            ));
        }

        if let Err(error) = file_operations.set_modification_time(
            &request.destination_peer,
            &request.relative_destination_file_path,
            request.winning_modification_time,
        ) {
            return StagedTransferTryOutcome::Failure(failure(
                StagedTransferFailurePhase::SetModTime,
                swap_old_state,
                error,
            ));
        }

        if target_existed {
            let timestamp = timestamp_generator.next_bak_timestamp();
            let bak_dir = join_path(
                &join_path(&destination.target_parent, ".kitchensync/BAK"),
                &timestamp,
            );
            let bak_path = join_path(&bak_dir, &destination.basename);

            if let Err(error) =
                file_operations.create_directory_all(&request.destination_peer, &bak_dir)
            {
                return StagedTransferTryOutcome::Failure(failure(
                    StagedTransferFailurePhase::ArchiveOld,
                    StagedTransferSwapOldState::Created,
                    error,
                ));
            }

            if let Err(error) = file_operations.rename_to_missing_path(
                &request.destination_peer,
                &destination.swap_old,
                &bak_path,
            ) {
                return StagedTransferTryOutcome::Failure(failure(
                    StagedTransferFailurePhase::ArchiveOld,
                    StagedTransferSwapOldState::Created,
                    error,
                ));
            }
        }

        if let Err(error) =
            file_operations.remove_empty_directory(&request.destination_peer, &destination.swap_dir)
        {
            return StagedTransferTryOutcome::Failure(failure(
                StagedTransferFailurePhase::Cleanup,
                swap_old_state,
                error,
            ));
        }

        if let Err(error) =
            file_operations.remove_empty_directory(&request.destination_peer, &destination.swap_root)
        {
            return StagedTransferTryOutcome::Failure(failure(
                StagedTransferFailurePhase::Cleanup,
                swap_old_state,
                error,
            ));
        }

        StagedTransferTryOutcome::Success
    }
}

struct DestinationPaths {
    target_parent: String,
    basename: String,
    encoded_basename: String,
    swap_root: String,
    swap_dir: String,
    swap_new: String,
    swap_old: String,
}

impl DestinationPaths {
    fn new(destination_path: &str) -> Self {
        let (target_parent, basename) = split_parent_basename(destination_path);
        let encoded_basename = utf8_percent_encode(&basename, PATH_SEGMENT_ENCODE_SET).to_string();
        let swap_root = join_path(&target_parent, ".kitchensync/SWAP");
        let swap_dir = join_path(&swap_root, &encoded_basename);
        let swap_new = join_path(&swap_dir, "new");
        let swap_old = join_path(&swap_dir, "old");

        Self {
            target_parent,
            basename,
            encoded_basename,
            swap_root,
            swap_dir,
            swap_new,
            swap_old,
        }
    }
}

fn split_parent_basename(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((parent, basename)) => (parent.to_string(), basename.to_string()),
        None => (String::new(), path.to_string()),
    }
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{}/{}", parent, child)
    }
}

fn stream_to_swap_new(
    request: &StagedTransferRequest,
    file_operations: &dyn StagedTransferFileOperations,
    destination: &DestinationPaths,
) -> Result<(), (StagedTransferFailurePhase, StagedTransferOperationError)> {
    let mut reader = file_operations
        .open_for_read(&request.source_peer, &request.relative_source_file_path)
        .map_err(|error| (StagedTransferFailurePhase::ReadSource, error))?;
    let mut writer = file_operations
        .create_new_for_write(&request.destination_peer, &destination.swap_new)
        .map_err(|error| (StagedTransferFailurePhase::WriteSwapNew, error))?;

    let mut buffer = [0_u8; COPY_BUFFER_SIZE];
    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|error| (StagedTransferFailurePhase::ReadSource, operation_error(error)))?;
        if bytes_read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..bytes_read])
            .map_err(|error| (StagedTransferFailurePhase::WriteSwapNew, operation_error(error)))?;
    }
    writer
        .flush()
        .map_err(|error| (StagedTransferFailurePhase::WriteSwapNew, operation_error(error)))?;

    Ok(())
}

fn operation_error(error: std::io::Error) -> StagedTransferOperationError {
    StagedTransferOperationError {
        transport_error_category: Some(match error.kind() {
            std::io::ErrorKind::NotFound => StagedTransferTransportErrorCategory::NotFound,
            std::io::ErrorKind::PermissionDenied => {
                StagedTransferTransportErrorCategory::PermissionDenied
            }
            _ => StagedTransferTransportErrorCategory::IoError,
        }),
        message: error.to_string(),
    }
}

fn cleanup_swap_new(
    file_operations: &dyn StagedTransferFileOperations,
    peer: &StagedTransferPeer,
    swap_new: &str,
) {
    let _ = file_operations.delete_file(peer, swap_new);
}

fn failure(
    phase: StagedTransferFailurePhase,
    swap_old_state: StagedTransferSwapOldState,
    error: StagedTransferOperationError,
) -> StagedTransferFailure {
    StagedTransferFailure {
        phase,
        swap_old_state,
        error,
    }
}

pub fn new() -> std::sync::Arc<dyn StagedTransfer> {
    Arc::new(StagedTransferImpl)
}
