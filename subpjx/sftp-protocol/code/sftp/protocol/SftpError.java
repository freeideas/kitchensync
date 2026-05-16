package sftp.protocol;

public enum SftpError {
    not_found,
    permission_denied,
    io_error,
    authentication_failed,
    host_key_rejected,
    invalid_path
}
