package sftp.protocol;

public record PoolEvent(String endpoint, int open_connections, int max_connections) {
}
