package sftp.protocol;

public final class SftpException extends Exception {
    private final SftpError category;

    public SftpException(SftpError category, String message) {
        super(message);
        this.category = category;
    }

    public SftpException(SftpError category, String message, Throwable cause) {
        super(message, cause);
        this.category = category;
    }

    public SftpError category() {
        return category;
    }
}
