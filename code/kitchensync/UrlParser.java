package kitchensync;

import java.net.URI;
import java.net.URISyntaxException;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;

final class UrlParser {
    private UrlParser() {
    }

    static PeerUrl parse(String raw, RunOptions defaults) {
        UrlConfig config = config(raw, defaults);
        String noQuery = stripQuery(raw);
        String lower = noQuery.toLowerCase(Locale.ROOT);
        if (lower.startsWith("sftp://")) {
            return parseSftp(noQuery, config);
        }
        if (lower.startsWith("file://")) {
            URI uri = URI.create(noQuery);
            Path path = Path.of(uri).toAbsolutePath().normalize();
            String normalized = "file://" + slash(path.toString());
            return new PeerUrl(normalized, "file", Optional.of(path), Optional.empty(), config);
        }
        Path path = Path.of(noQuery).toAbsolutePath().normalize();
        String normalized = "file://" + slash(path.toString());
        return new PeerUrl(normalized, "file", Optional.of(path), Optional.empty(), config);
    }

    private static PeerUrl parseSftp(String text, UrlConfig config) {
        try {
            URI uri = new URI(text);
            String host = uri.getHost();
            if (host == null || host.isBlank()) {
                throw new CliParser.ValidationException("Invalid sftp URL: " + text);
            }
            String user = System.getProperty("user.name", "");
            Optional<String> password = Optional.empty();
            String userInfo = uri.getRawUserInfo();
            if (userInfo != null && !userInfo.isBlank()) {
                int colon = userInfo.indexOf(':');
                if (colon >= 0) {
                    user = decodeUserInfo(userInfo.substring(0, colon));
                    password = Optional.of(decodeUserInfo(userInfo.substring(colon + 1)));
                } else {
                    user = decodeUserInfo(userInfo);
                }
            }
            int port = uri.getPort() == -1 ? 22 : uri.getPort();
            String path = collapseSlashes(uri.getRawPath() == null || uri.getRawPath().isBlank() ? "/" : decodePath(uri.getRawPath()));
            if (path.length() > 1 && path.endsWith("/")) {
                path = path.substring(0, path.length() - 1);
            }
            host = host.toLowerCase(Locale.ROOT);
            String authority = user + "@" + host + (port == 22 ? "" : ":" + port);
            String normalized = "sftp://" + authority + path;
            SftpParts parts = new SftpParts(user, password, host, port, path);
            return new PeerUrl(normalized, "sftp", Optional.empty(), Optional.of(parts), config);
        } catch (URISyntaxException ex) {
            throw new CliParser.ValidationException("Invalid sftp URL: " + text);
        }
    }

    private static UrlConfig config(String raw, RunOptions defaults) {
        Map<String, String> query = query(raw);
        int mc = positive(query.getOrDefault("mc", Integer.toString(defaults.maxConnections)), "mc");
        int ct = positive(query.getOrDefault("ct", Integer.toString(defaults.connectTimeoutSeconds)), "ct");
        int ka = positive(query.getOrDefault("ka", Integer.toString(defaults.keepAliveSeconds)), "ka");
        return new UrlConfig(mc, ct, ka);
    }

    private static int positive(String text, String key) {
        try {
            int value = Integer.parseInt(text);
            if (value <= 0) {
                throw new NumberFormatException();
            }
            return value;
        } catch (NumberFormatException ex) {
            throw new CliParser.ValidationException("Invalid URL setting " + key + "=" + text);
        }
    }

    private static Map<String, String> query(String raw) {
        int question = raw.indexOf('?');
        Map<String, String> result = new HashMap<>();
        if (question < 0 || question == raw.length() - 1) {
            return result;
        }
        for (String part : raw.substring(question + 1).split("&")) {
            int equals = part.indexOf('=');
            if (equals > 0) {
                result.put(part.substring(0, equals), part.substring(equals + 1));
            }
        }
        return result;
    }

    private static String stripQuery(String raw) {
        int question = raw.indexOf('?');
        return question >= 0 ? raw.substring(0, question) : raw;
    }

    private static String collapseSlashes(String path) {
        return path.replaceAll("/{2,}", "/");
    }

    private static String decodePath(String raw) {
        return decode(raw).replace('\\', '/');
    }

    private static String decodeUserInfo(String raw) {
        return decode(raw, true);
    }

    private static String decode(String raw) {
        return decode(raw, false);
    }

    private static String decode(String raw, boolean decodeReserved) {
        StringBuilder out = new StringBuilder();
        for (int i = 0; i < raw.length(); i++) {
            char c = raw.charAt(i);
            if (c == '%' && i + 2 < raw.length()) {
                int value = Integer.parseInt(raw.substring(i + 1, i + 3), 16);
                char decoded = (char) value;
                if (decodeReserved || isUnreserved(decoded) || raw.indexOf(':') >= 0) {
                    out.append(decoded);
                } else {
                    out.append('%').append(raw, i + 1, i + 3);
                }
                i += 2;
            } else {
                out.append(c);
            }
        }
        return out.toString();
    }

    private static boolean isUnreserved(char c) {
        return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')
                || c == '-' || c == '.' || c == '_' || c == '~';
    }

    private static String slash(String path) {
        return path.replace('\\', '/');
    }
}
