package kitchensync;

import java.util.Optional;

record SftpParts(String user, Optional<String> password, String host, int port, String path) {
    String endpointKey() {
        return user + "@" + host + ":" + port;
    }
}
