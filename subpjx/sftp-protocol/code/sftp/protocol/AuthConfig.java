package sftp.protocol;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Objects;
import java.util.Optional;

public record AuthConfig(
        Path known_hosts_path,
        Optional<Path> ssh_agent_socket,
        List<Path> private_key_paths) {
    public AuthConfig {
        Objects.requireNonNull(known_hosts_path, "known_hosts_path");
        Objects.requireNonNull(ssh_agent_socket, "ssh_agent_socket");
        Objects.requireNonNull(private_key_paths, "private_key_paths");
        private_key_paths = List.copyOf(private_key_paths);
    }

    public static AuthConfig defaults() {
        Path home = Path.of(System.getProperty("user.home"));
        Optional<Path> agent = Optional.ofNullable(System.getenv("SSH_AUTH_SOCK"))
                .filter(s -> !s.isBlank())
                .map(Path::of);
        List<Path> keys = new ArrayList<>();
        keys.add(home.resolve(".ssh").resolve("id_ed25519"));
        keys.add(home.resolve(".ssh").resolve("id_ecdsa"));
        keys.add(home.resolve(".ssh").resolve("id_rsa"));
        return new AuthConfig(home.resolve(".ssh").resolve("known_hosts"), agent, keys);
    }
}
