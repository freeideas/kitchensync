package bounded.resource.pool;

import java.time.Duration;
import java.util.Objects;

public record PoolSettings(int max_resources, Duration idle_keep_alive_ttl) {
    public PoolSettings {
        if (max_resources <= 0) {
            throw new IllegalArgumentException("max_resources must be positive");
        }
        Objects.requireNonNull(idle_keep_alive_ttl, "idle_keep_alive_ttl");
        if (!idle_keep_alive_ttl.isPositive()) {
            throw new IllegalArgumentException("idle_keep_alive_ttl must be positive");
        }
    }
}
