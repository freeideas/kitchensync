package ssh.sftp.session;

public enum Failure {
    NOT_FOUND("not_found"),
    PERMISSION_DENIED("permission_denied"),
    IO_FAILURE("io_failure");

    private final String code;

    Failure(String code) { this.code = code; }

    public String code() { return code; }
}
