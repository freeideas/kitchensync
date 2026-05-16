package staged.file.transfer;

public enum TransferError {
    invalid_path,
    invalid_timestamp,
    invalid_transfer_id,
    invalid_settings,
    same_source_and_destination,
    not_found,
    permission_denied,
    io_error,
    displacement_failed,
    rename_failed,
    set_mod_time_failed,
    cleanup_incomplete
}
