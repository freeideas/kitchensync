package kitchensync;

import java.nio.file.Path;
import java.util.Optional;

import url.parser.ParseContext;
import url.parser.ParsedUrl;
import url.parser.PeerUrlParser;
import url.parser.UrlParseException;
import url.parser.UrlSettings;
import url.parser.UrlScheme;

final class UrlParser {
    private UrlParser() {
    }

    static PeerUrl parse(String raw, RunOptions defaults) {
        try {
            ParsedUrl parsed = PeerUrlParser.parse_url(raw, context());
            UrlConfig config = config(parsed.settings(), defaults);
            if (parsed.scheme() == UrlScheme.file) {
                Path path = Path.of(parsed.path());
                return new PeerUrl(fileIdentity(path), "file", Optional.of(path), Optional.empty(), config);
            }
            SftpParts parts = new SftpParts(
                    parsed.userValue().orElseThrow(),
                    parsed.passwordValue(),
                    parsed.hostValue().orElseThrow(),
                    parsed.portValue().orElse(22),
                    parsed.path());
            return new PeerUrl(parsed.canonical_identity(), "sftp", Optional.empty(), Optional.of(parts), config);
        } catch (UrlParseException ex) {
            throw new CliParser.ValidationException("Invalid URL: " + raw);
        }
    }

    private static ParseContext context() {
        return new ParseContext(Path.of("").toAbsolutePath().normalize().toString(),
                System.getProperty("user.name", ""));
    }

    private static UrlConfig config(UrlSettings settings, RunOptions defaults) {
        return new UrlConfig(
                settings.maxConnections().orElse(defaults.maxConnections),
                settings.connectTimeoutSeconds().orElse(defaults.connectTimeoutSeconds),
                settings.idleKeepAliveSeconds().orElse(defaults.keepAliveSeconds));
    }

    private static String fileIdentity(Path path) {
        String normalizedPath = path.toAbsolutePath().normalize().toString().replace('\\', '/');
        if (normalizedPath.length() >= 2 && normalizedPath.charAt(1) == ':') {
            normalizedPath = Character.toUpperCase(normalizedPath.charAt(0)) + normalizedPath.substring(1);
        }
        if (!normalizedPath.startsWith("/")) {
            normalizedPath = "/" + normalizedPath;
        }
        return "file://" + normalizedPath;
    }
}
