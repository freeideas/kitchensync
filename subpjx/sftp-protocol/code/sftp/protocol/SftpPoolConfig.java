package sftp.protocol;

public record SftpPoolConfig(int maxConnections, double connectTimeoutSeconds, double idleKeepaliveSeconds) {
    public static SftpPoolConfig defaults() {
        return new SftpPoolConfig(10, 30.0, 30.0);
    }
}
