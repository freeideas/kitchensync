package sftp.protocol;

public class SftpPermissionDeniedException extends SftpException {
    public SftpPermissionDeniedException(String message) { super(message); }
    public SftpPermissionDeniedException(String message, Throwable cause) { super(message, cause); }
}
