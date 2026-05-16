package url.parser;

import java.util.Optional;
import java.util.OptionalInt;

public record ParsedUrl(
        UrlScheme scheme,
        String canonical_identity,
        UrlSettings settings,
        String path,
        String user,
        String password,
        String host,
        Integer port,
        String endpoint_key) {
    public Optional<String> userValue() {
        return Optional.ofNullable(user);
    }

    public Optional<String> passwordValue() {
        return Optional.ofNullable(password);
    }

    public Optional<String> hostValue() {
        return Optional.ofNullable(host);
    }

    public OptionalInt portValue() {
        return port == null ? OptionalInt.empty() : OptionalInt.of(port);
    }

    public Optional<String> endpointKey() {
        return Optional.ofNullable(endpoint_key);
    }
}
