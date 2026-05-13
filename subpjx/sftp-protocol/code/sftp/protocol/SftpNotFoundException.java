package sftp.protocol;

public class SftpNotFoundException extends SftpException {
    public SftpNotFoundException(String message) { super(message); }
    public SftpNotFoundException(String message, Throwable cause) { super(message, cause); }
}
