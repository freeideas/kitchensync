package sftp.protocol;

import java.util.Objects;
import java.util.Optional;
import java.util.Locale;

public record SftpLocation(
        String user,
        Optional<String> password,
        String host,
        int port,
        String root_path) {
    public SftpLocation {
        Objects.requireNonNull(user, "user");
        Objects.requireNonNull(password, "password");
        Objects.requireNonNull(host, "host");
        Objects.requireNonNull(root_path, "root_path");
        if (user.isBlank()) {
            throw new IllegalArgumentException("user is required");
        }
        if (host.isBlank()) {
            throw new IllegalArgumentException("host is required");
        }
        host = host.toLowerCase(Locale.ROOT);
        port = port == 0 ? 22 : port;
        if (port <= 0) {
            throw new IllegalArgumentException("port must be positive");
        }
        if (!root_path.startsWith("/")) {
            throw new IllegalArgumentException("root_path must be absolute");
        }
    }

    public String endpointKey() {
        return user + "@" + host + ":" + port;
    }
}
