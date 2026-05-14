package url.normalizer;

import java.net.URI;
import java.net.URISyntaxException;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.Locale;
import java.util.regex.Pattern;

public final class UrlNormalizer {

    private static final Pattern WINDOWS_ABSOLUTE_PATH = Pattern.compile("(?i)^[a-z]:[\\\\/].*");
    private static final Pattern WINDOWS_LEADING_ABSOLUTE_PATH = Pattern.compile("(?i)^/[a-z]:[\\\\/].*");
    private static final Pattern SCHEME_PREFIX = Pattern.compile("(?i)^[a-z][a-z0-9+.-]*:.*");

    private UrlNormalizer() {}

    public static NormalizedUrl normalizeUrl(String text, ParseContext context) throws UrlNormalizerError {
        return normalize_url(text, context);
    }

    public static NormalizedUrl normalize_url(String text, ParseContext context) throws UrlNormalizerError {
        if (text == null || context == null) {
            throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_URL);
        }
        if (context.current_working_directory() == null || context.current_os_user() == null) {
            throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_URL);
        }

        String candidate = text.strip();
        if (candidate.isEmpty()) {
            throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_URL);
        }

        if (isWindowsAbsolutePath(candidate) || !looksLikeScheme(candidate)) {
            return normalizeFile(candidate, context);
        }

        URI uri;
        try {
            uri = new URI(candidate);
        } catch (URISyntaxException e) {
            throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_URL);
        }

        if (uri.getScheme() == null) {
            return normalizeFile(candidate, context);
        }

        String scheme = uri.getScheme().toLowerCase(Locale.ROOT);
        return switch (scheme) {
            case "file" -> normalizeFile(uri, context);
            case "sftp" -> normalizeSftp(uri, context);
            default -> throw new UrlNormalizerError(UrlNormalizerError.Code.UNSUPPORTED_SCHEME);
        };
    }

    private static boolean looksLikeScheme(String text) {
        return SCHEME_PREFIX.matcher(text).matches();
    }

    private static boolean isWindowsAbsolutePath(String text) {
        return WINDOWS_ABSOLUTE_PATH.matcher(text).matches();
    }

    private static boolean isWindowsLeadingAbsolutePath(String text) {
        return WINDOWS_LEADING_ABSOLUTE_PATH.matcher(text).matches();
    }

    private static NormalizedUrl normalizeFile(String text, ParseContext context) throws UrlNormalizerError {
        String cleaned = removeQueryAndFragment(text);
        String normalizedPath = normalizePath(cleaned);
        if (normalizedPath.isEmpty()) {
            throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_URL);
        }
        String absolutePath = resolveFilePath(normalizedPath, context.current_working_directory());
        return new NormalizedUrl("file://" + absolutePath);
    }

    private static NormalizedUrl normalizeFile(URI uri, ParseContext context) throws UrlNormalizerError {
        String path = uri.getRawPath();
        if (path == null) {
            path = "";
        }
        String normalizedPath = normalizePath(path);
        if (normalizedPath.isEmpty()) {
            normalizedPath = "/";
        }
        String absolutePath = resolveFilePath(normalizedPath, context.current_working_directory());
        return new NormalizedUrl("file://" + absolutePath);
    }

    private static NormalizedUrl normalizeSftp(URI uri, ParseContext context) throws UrlNormalizerError {
        String rawPath = uri.getRawPath();
        if (rawPath == null) {
            rawPath = "";
        }
        String normalizedPath = normalizePath(rawPath);
        if ("/".equals(normalizedPath)) {
            normalizedPath = "";
        }

        String host = uri.getHost();
        if (host == null || host.isBlank()) {
            host = parseHostFromAuthority(uri.getRawAuthority());
            if (host == null || host.isBlank()) {
                throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_URL);
            }
        }

        String user = uri.getUserInfo();
        if (user == null || user.isBlank()) {
            user = context.current_os_user();
        }

        Integer port = parsePort(uri);
        if (port != null && port == 22) {
            port = null;
        }

        StringBuilder result = new StringBuilder();
        result.append("sftp://");
        result.append(user);
        result.append("@");
        result.append(host.toLowerCase(Locale.ROOT));
        if (port != null) {
            result.append(":").append(port);
        }
        result.append(normalizedPath);
        return new NormalizedUrl(result.toString());
    }

    private static String parseHostFromAuthority(String rawAuthority) {
        if (rawAuthority == null || rawAuthority.isBlank()) {
            return null;
        }
        String authority = rawAuthority;
        int at = authority.lastIndexOf('@');
        if (at >= 0) {
            authority = authority.substring(at + 1);
        }
        if (authority.startsWith("[")) {
            int close = authority.indexOf(']');
            if (close < 0) {
                return null;
            }
            return authority.substring(1, close);
        }
        int colon = authority.lastIndexOf(':');
        if (colon < 0) {
            return authority;
        }
        return authority.substring(0, colon);
    }

    private static Integer parsePort(URI uri) throws UrlNormalizerError {
        String rawAuthority = uri.getRawAuthority();
        if (rawAuthority == null || rawAuthority.isBlank()) {
            return null;
        }

        String authority = rawAuthority;
        int at = authority.lastIndexOf('@');
        if (at >= 0) {
            authority = authority.substring(at + 1);
        }

        if (authority.startsWith("[")) {
            int close = authority.indexOf(']');
            if (close < 0 || close == authority.length() - 1) {
                return null;
            }
            if (authority.charAt(close + 1) != ':') {
                return null;
            }
            String portText = authority.substring(close + 2);
            if (portText.isBlank()) {
                throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_PORT);
            }
            return parsePortText(portText);
        }

        int colon = authority.lastIndexOf(':');
        if (colon < 0) {
            return null;
        }
        String portText = authority.substring(colon + 1);
        if (portText.isBlank()) {
            throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_PORT);
        }
        return parsePortText(portText);
    }

    private static Integer parsePortText(String portText) throws UrlNormalizerError {
        if (!portText.matches("\\d+")) {
            throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_PORT);
        }
        long parsed = Long.parseLong(portText);
        if (parsed < 0 || parsed > 65535) {
            throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_PORT);
        }
        return (int) parsed;
    }

    private static String resolveFilePath(String path, String cwd) {
        String prepared = path.replace('\\', '/');
        if (prepared.isBlank()) {
            return "/";
        }
        if (isWindowsLeadingAbsolutePath(prepared)) {
            prepared = prepared.substring(1);
        }

        Path base = Paths.get(cwd).toAbsolutePath().normalize();
        Path candidate = isAbsoluteFilePath(prepared)
                ? Paths.get(prepared)
                : base.resolve(prepared).normalize();
        return candidate.toString().replace('\\', '/');
    }

    private static boolean isAbsoluteFilePath(String path) {
        return path.startsWith("/") || isWindowsAbsolutePath(path);
    }

    private static String normalizePath(String rawPath) throws UrlNormalizerError {
        String decoded = percentDecodeUnreserved(rawPath);
        String collapsed = decoded.replace('\\', '/').replaceAll("/{2,}", "/");
        return trimTrailingSlash(collapsed);
    }

    private static String trimTrailingSlash(String path) {
        if (path.length() <= 1) {
            return path;
        }
        int end = path.length();
        while (end > 1 && path.charAt(end - 1) == '/') {
            end--;
        }
        return path.substring(0, end);
    }

    private static String percentDecodeUnreserved(String input) throws UrlNormalizerError {
        StringBuilder decoded = new StringBuilder(input.length());
        for (int i = 0; i < input.length(); i++) {
            char c = input.charAt(i);
            if (c != '%') {
                decoded.append(c);
                continue;
            }
            if (i + 2 >= input.length()) {
                throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_PERCENT_ENCODING);
            }
            String hex = input.substring(i + 1, i + 3);
            int value;
            try {
                value = Integer.parseInt(hex, 16);
            } catch (NumberFormatException e) {
                throw new UrlNormalizerError(UrlNormalizerError.Code.INVALID_PERCENT_ENCODING);
            }
            char decodedChar = (char) value;
            if (isUnreserved(decodedChar)) {
                decoded.append(decodedChar);
            } else {
                decoded.append('%').append(hex);
            }
            i += 2;
        }
        return decoded.toString();
    }

    private static boolean isUnreserved(char c) {
        return Character.isLetterOrDigit(c) || c == '-' || c == '.' || c == '_' || c == '~';
    }

    private static String removeQueryAndFragment(String text) {
        int query = text.indexOf('?');
        int fragment = text.indexOf('#');
        int end = text.length();
        if (query >= 0) {
            end = query;
        }
        if (fragment >= 0 && fragment < end) {
            end = fragment;
        }
        return text.substring(0, end);
    }
}

