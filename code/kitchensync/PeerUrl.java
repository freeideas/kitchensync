package kitchensync;

import java.nio.file.Path;
import java.util.Optional;

record PeerUrl(
        String normalized,
        String scheme,
        Optional<Path> localPath,
        Optional<SftpParts> sftp,
        UrlConfig config) {
}
