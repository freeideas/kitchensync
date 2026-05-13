package sftp.protocol;

public class SftpIoException extends SftpException {
    public SftpIoException(String message) { super(message); }
    public SftpIoException(String message, Throwable cause) { super(message, cause); }
}
