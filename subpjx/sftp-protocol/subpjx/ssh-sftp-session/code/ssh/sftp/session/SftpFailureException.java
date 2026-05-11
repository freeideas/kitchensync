package ssh.sftp.session;

public class SftpFailureException extends Exception {
    public final Failure failure;
    public SftpFailureException(Failure f) { this.failure = f; }
}
