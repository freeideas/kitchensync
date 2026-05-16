package sftp.protocol;

import java.time.Duration;
import java.util.Objects;

public record SftpSettings(
        int max_connections,
        Duration connect_timeout,
        Duration idle_keep_alive_ttl) {
    public SftpSettings {
        Objects.requireNonNull(connect_timeout, "connect_timeout");
        Objects.requireNonNull(idle_keep_alive_ttl, "idle_keep_alive_ttl");
        if (max_connections <= 0) {
            throw new IllegalArgumentException("max_connections must be positive");
        }
        if (connect_timeout.isZero() || connect_timeout.isNegative()) {
            throw new IllegalArgumentException("connect_timeout must be positive");
        }
        if (idle_keep_alive_ttl.isZero() || idle_keep_alive_ttl.isNegative()) {
            throw new IllegalArgumentException("idle_keep_alive_ttl must be positive");
        }
    }
}
