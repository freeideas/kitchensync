package sftp.protocol;

public class SftpException extends RuntimeException {
    public SftpException(String message) { super(message); }
    public SftpException(String message, Throwable cause) { super(message, cause); }
}
