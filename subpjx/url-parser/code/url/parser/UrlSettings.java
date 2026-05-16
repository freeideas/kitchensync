package url.parser;

import java.util.OptionalInt;

public record UrlSettings(Integer max_connections, Integer connect_timeout_seconds, Integer idle_keep_alive_seconds) {
    public OptionalInt maxConnections() {
        return max_connections == null ? OptionalInt.empty() : OptionalInt.of(max_connections);
    }

    public OptionalInt connectTimeoutSeconds() {
        return connect_timeout_seconds == null ? OptionalInt.empty() : OptionalInt.of(connect_timeout_seconds);
    }

    public OptionalInt idleKeepAliveSeconds() {
        return idle_keep_alive_seconds == null ? OptionalInt.empty() : OptionalInt.of(idle_keep_alive_seconds);
    }
}
