package url.parser;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;

public final class PeerUrlParser {
    private PeerUrlParser() {
    }

    public static ParsedPeer parse_peer_operand(String text, ParseContext context) {
        validateContext(context);
        if (text == null || text.isEmpty()) {
            throw new UrlParseException(ParseErrorCategory.empty_operand, "");
        }

        PeerRole role = PeerRole.normal;
        String operand = text;
        if (operand.charAt(0) == '+' || operand.charAt(0) == '-') {
            role = operand.charAt(0) == '+' ? PeerRole.canon : PeerRole.subordinate;
            operand = operand.substring(1);
            if (operand.isEmpty()) {
                throw new UrlParseException(ParseErrorCategory.empty_operand, "");
            }
            if (operand.charAt(0) == '+' || operand.charAt(0) == '-') {
                throw new UrlParseException(ParseErrorCategory.invalid_role_prefix, "more than one role prefix");
            }
        }

        if (operand.startsWith("[") || operand.endsWith("]")) {
            return new ParsedPeer(role, parseFallbackGroup(operand, context));
        }
        return new ParsedPeer(role, List.of(parse_url(operand, context)));
    }

    public static ParsedPeer parsePeerOperand(String text, ParseContext context) {
        return parse_peer_operand(text, context);
    }

    public static ParsedUrl parse_url(String text, ParseContext context) {
        validateContext(context);
        if (text == null || text.isEmpty()) {
            throw new UrlParseException(ParseErrorCategory.empty_operand, "");
        }
        if (text.charAt(0) == '+' || text.charAt(0) == '-') {
            throw new UrlParseException(ParseErrorCategory.invalid_role_prefix, "URL candidates do not accept roles or brackets");
        }
        if (text.indexOf('[') >= 0 || text.indexOf(']') >= 0) {
            throw new UrlParseException(ParseErrorCategory.invalid_fallback_group, "URL candidates do not accept brackets");
        }
        validatePercentEncoding(text);

        String noFragment = stripFragment(text);
        SchemeMatch scheme = schemeOf(noFragment);
        if (scheme.name == null) {
            return parseBarePath(noFragment, context);
        }
        return switch (scheme.name) {
            case "file" -> parseFileUrl(noFragment);
            case "sftp" -> parseSftpUrl(noFragment, context);
            default -> throw new UrlParseException(ParseErrorCategory.unsupported_scheme, scheme.name);
        };
    }

    public static ParsedUrl parseUrl(String text, ParseContext context) {
        return parse_url(text, context);
    }

    public static String normalize_identity(String text, ParseContext context) {
        return parse_url(text, context).canonical_identity();
    }

    public static String normalizeIdentity(String text, ParseContext context) {
        return normalize_identity(text, context);
    }

    private static List<ParsedUrl> parseFallbackGroup(String text, ParseContext context) {
        if (!text.startsWith("[") || !text.endsWith("]")) {
            throw new UrlParseException(ParseErrorCategory.invalid_fallback_group, "unbalanced brackets");
        }
        String body = text.substring(1, text.length() - 1);
        if (body.isEmpty()) {
            throw new UrlParseException(ParseErrorCategory.invalid_fallback_group, "empty group");
        }

        List<ParsedUrl> urls = new ArrayList<>();
        int start = 0;
        for (int index = 0; index <= body.length(); index++) {
            if (index == body.length() || body.charAt(index) == ',') {
                String candidate = body.substring(start, index);
                if (candidate.isEmpty()) {
                    throw new UrlParseException(ParseErrorCategory.invalid_fallback_group, "empty candidate");
                }
                if (candidate.charAt(0) == '+' || candidate.charAt(0) == '-') {
                    throw new UrlParseException(ParseErrorCategory.invalid_role_prefix, "role prefix inside fallback group");
                }
                if (candidate.indexOf('[') >= 0 || candidate.indexOf(']') >= 0) {
                    throw new UrlParseException(ParseErrorCategory.invalid_fallback_group, "nested group");
                }
                urls.add(parse_url(candidate, context));
                start = index + 1;
            } else if (body.charAt(index) == '[' || body.charAt(index) == ']') {
                throw new UrlParseException(ParseErrorCategory.invalid_fallback_group, "nested group");
            }
        }
        return urls;
    }

    private static ParsedUrl parseBarePath(String text, ParseContext context) {
        SplitQuery split = splitQuery(text);
        UrlSettings settings = parseSettings(split.query);
        String path = normalizeLocalPath(split.main, context);
        return new ParsedUrl(UrlScheme.file, fileIdentity(path), settings, path, null, null, null, null, null);
    }

    private static ParsedUrl parseFileUrl(String text) {
        SplitQuery split = splitQuery(text);
        UrlSettings settings = parseSettings(split.query);
        if (!split.main.regionMatches(true, 0, "file:///", 0, 8)) {
            throw new UrlParseException(ParseErrorCategory.invalid_file_url, "file URL must use file:///");
        }
        String rawPath = split.main.substring(8);
        if (rawPath.isEmpty()) {
            rawPath = "/";
        }
        if (isWindowsDrivePath(rawPath)) {
            rawPath = rawPath.substring(0, 1).toLowerCase(Locale.ROOT) + rawPath.substring(1);
        } else if (rawPath.startsWith("/") && isWindowsDrivePath(rawPath.substring(1))) {
            rawPath = rawPath.substring(1);
        } else {
            rawPath = "/" + rawPath;
        }
        String path = normalizeAbsolutePath(rawPath.replace('\\', '/'));
        return new ParsedUrl(UrlScheme.file, fileIdentity(path), settings, path, null, null, null, null, null);
    }

    private static ParsedUrl parseSftpUrl(String text, ParseContext context) {
        SplitQuery split = splitQuery(text);
        UrlSettings settings = parseSettings(split.query);
        if (!split.main.regionMatches(true, 0, "sftp://", 0, 7)) {
            throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "");
        }
        String rest = split.main.substring(7);
        int slash = rest.indexOf('/');
        if (slash < 0) {
            throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "missing absolute path");
        }
        String authority = rest.substring(0, slash);
        String rawPath = rest.substring(slash);
        if (authority.isEmpty() || rawPath.isEmpty() || rawPath.charAt(0) != '/') {
            throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "missing host or absolute path");
        }

        String userInfo = null;
        String hostPort = authority;
        int at = authority.lastIndexOf('@');
        if (at >= 0) {
            userInfo = authority.substring(0, at);
            hostPort = authority.substring(at + 1);
            if (userInfo.isEmpty() || userInfo.indexOf('@') >= 0) {
                throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "invalid user info");
            }
        }

        HostPort parsedHost = parseHostPort(hostPort);
        String user = context.current_os_user();
        String password = null;
        if (userInfo != null) {
            int colon = userInfo.indexOf(':');
            if (colon != userInfo.lastIndexOf(':')) {
                throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "invalid user info");
            }
            if (colon >= 0) {
                user = userInfo.substring(0, colon);
                password = decodeAll(userInfo.substring(colon + 1));
            } else {
                user = userInfo;
            }
            if (user.isEmpty()) {
                throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "empty user");
            }
        }

        String path = normalizeAbsolutePath(rawPath);
        String identity = "sftp://" + decodeUnreserved(user) + "@" + parsedHost.host;
        if (parsedHost.port != 22) {
            identity += ":" + parsedHost.port;
        }
        identity += path;
        String endpointKey = user + "@" + parsedHost.host + ":" + parsedHost.port;
        return new ParsedUrl(UrlScheme.sftp, identity, settings, path, user, password, parsedHost.host, parsedHost.port, endpointKey);
    }

    private static HostPort parseHostPort(String hostPort) {
        if (hostPort.isEmpty() || hostPort.startsWith(":")) {
            throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "missing host");
        }
        String host = hostPort;
        int port = 22;
        int colon = hostPort.lastIndexOf(':');
        if (colon >= 0) {
            if (colon != hostPort.indexOf(':')) {
                throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "invalid host");
            }
            host = hostPort.substring(0, colon);
            String portText = hostPort.substring(colon + 1);
            if (portText.isEmpty()) {
                throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "missing port");
            }
            try {
                port = Integer.parseInt(portText);
            } catch (NumberFormatException ex) {
                throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "invalid port");
            }
            if (port < 1 || port > 65535) {
                throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "invalid port");
            }
        }
        if (host.isEmpty()) {
            throw new UrlParseException(ParseErrorCategory.invalid_sftp_url, "missing host");
        }
        return new HostPort(decodeUnreserved(host).toLowerCase(Locale.ROOT), port);
    }

    private static UrlSettings parseSettings(String query) {
        if (query == null || query.isEmpty()) {
            return new UrlSettings(null, null, null);
        }
        Integer mc = null;
        Integer ct = null;
        Integer ka = null;
        Set<String> seen = new HashSet<>();
        for (String part : query.split("&", -1)) {
            int equals = part.indexOf('=');
            String key = equals >= 0 ? part.substring(0, equals) : part;
            String value = equals >= 0 ? part.substring(equals + 1) : "";
            if (!key.equals("mc") && !key.equals("ct") && !key.equals("ka")) {
                throw new UrlParseException(ParseErrorCategory.invalid_setting, "unknown setting " + key);
            }
            if (!seen.add(key)) {
                throw new UrlParseException(ParseErrorCategory.invalid_setting, "duplicate setting " + key);
            }
            int parsed;
            try {
                if (value.isEmpty() || value.charAt(0) == '+') {
                    throw new NumberFormatException();
                }
                parsed = Integer.parseInt(value);
            } catch (NumberFormatException ex) {
                throw new UrlParseException(ParseErrorCategory.invalid_setting, "invalid value for " + key);
            }
            if (parsed <= 0) {
                throw new UrlParseException(ParseErrorCategory.invalid_setting, "non-positive value for " + key);
            }
            if (key.equals("mc")) {
                mc = parsed;
            } else if (key.equals("ct")) {
                ct = parsed;
            } else {
                ka = parsed;
            }
        }
        return new UrlSettings(mc, ct, ka);
    }

    private static String normalizeLocalPath(String raw, ParseContext context) {
        String path = raw.replace('\\', '/');
        if (isWindowsDrivePath(path)) {
            return normalizeAbsolutePath(path.substring(0, 1).toLowerCase(Locale.ROOT) + path.substring(1));
        }
        if (path.startsWith("/")) {
            return normalizeAbsolutePath(path);
        }
        return normalizeAbsolutePath(trimTrailingSlash(context.current_working_directory().replace('\\', '/')) + "/" + path);
    }

    private static String normalizeAbsolutePath(String raw) {
        String decoded = decodeUnreserved(raw.replace('\\', '/'));
        String collapsed = collapseSlashes(decoded);
        return trimTrailingSlash(removeDotSegments(collapsed));
    }

    private static String fileIdentity(String path) {
        return path.startsWith("/") ? "file://" + path : "file:///" + path;
    }

    private static String removeDotSegments(String path) {
        boolean posixRoot = path.startsWith("/");
        String prefix = "";
        String rest = path;
        if (isWindowsDrivePath(path)) {
            prefix = path.substring(0, 3);
            rest = path.substring(3);
        }
        List<String> parts = new ArrayList<>();
        for (String part : rest.split("/", -1)) {
            if (part.isEmpty() || part.equals(".")) {
                continue;
            }
            if (part.equals("..")) {
                if (!parts.isEmpty() && !parts.get(parts.size() - 1).equals("..")) {
                    parts.remove(parts.size() - 1);
                } else if (!posixRoot && prefix.isEmpty()) {
                    parts.add(part);
                }
            } else {
                parts.add(part);
            }
        }
        String joined = String.join("/", parts);
        if (!prefix.isEmpty()) {
            return joined.isEmpty() ? trimTrailingSlash(prefix) : prefix + joined;
        }
        if (posixRoot) {
            return joined.isEmpty() ? "/" : "/" + joined;
        }
        return joined;
    }

    private static String collapseSlashes(String value) {
        StringBuilder out = new StringBuilder(value.length());
        boolean previousSlash = false;
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            if (c == '/') {
                if (!previousSlash) {
                    out.append(c);
                }
                previousSlash = true;
            } else {
                out.append(c);
                previousSlash = false;
            }
        }
        return out.toString();
    }

    private static String trimTrailingSlash(String path) {
        while (path.length() > 1 && !isWindowsDriveRoot(path) && path.endsWith("/")) {
            path = path.substring(0, path.length() - 1);
        }
        return path;
    }

    private static boolean isWindowsDriveRoot(String path) {
        return path.length() == 3 && Character.isLetter(path.charAt(0)) && path.charAt(1) == ':' && path.charAt(2) == '/';
    }

    private static void validateContext(ParseContext context) {
        if (context == null || context.current_os_user().isEmpty()) {
            throw new UrlParseException(ParseErrorCategory.invalid_context, "empty current user");
        }
        String cwd = context.current_working_directory().replace('\\', '/');
        if (cwd.isEmpty() || (!cwd.startsWith("/") && !isWindowsDrivePath(cwd))) {
            throw new UrlParseException(ParseErrorCategory.invalid_context, "current working directory is not absolute");
        }
    }

    private static SplitQuery splitQuery(String text) {
        int query = text.indexOf('?');
        if (query < 0) {
            return new SplitQuery(text, null);
        }
        return new SplitQuery(text.substring(0, query), text.substring(query + 1));
    }

    private static String stripFragment(String text) {
        int fragment = text.indexOf('#');
        return fragment < 0 ? text : text.substring(0, fragment);
    }

    private static SchemeMatch schemeOf(String text) {
        int colon = text.indexOf(':');
        if (colon <= 0) {
            return new SchemeMatch(null);
        }
        if (colon == 1 && text.length() > 2 && (text.charAt(2) == '/' || text.charAt(2) == '\\')) {
            return new SchemeMatch(null);
        }
        for (int i = 0; i < colon; i++) {
            char c = text.charAt(i);
            boolean ok = i == 0 ? Character.isLetter(c) : Character.isLetterOrDigit(c) || c == '+' || c == '-' || c == '.';
            if (!ok) {
                return new SchemeMatch(null);
            }
        }
        return new SchemeMatch(text.substring(0, colon).toLowerCase(Locale.ROOT));
    }

    private static boolean isWindowsDrivePath(String path) {
        return path.length() >= 3
                && Character.isLetter(path.charAt(0))
                && path.charAt(1) == ':'
                && path.charAt(2) == '/';
    }

    private static void validatePercentEncoding(String text) {
        for (int i = 0; i < text.length(); i++) {
            if (text.charAt(i) == '%') {
                if (i + 2 >= text.length() || hex(text.charAt(i + 1)) < 0 || hex(text.charAt(i + 2)) < 0) {
                    throw new UrlParseException(ParseErrorCategory.invalid_percent_encoding, "");
                }
                i += 2;
            }
        }
    }

    private static String decodeUnreserved(String text) {
        StringBuilder out = new StringBuilder(text.length());
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c == '%') {
                int value = hex(text.charAt(i + 1)) * 16 + hex(text.charAt(i + 2));
                char decoded = (char) value;
                if (isUnreserved(decoded)) {
                    out.append(decoded);
                } else {
                    out.append('%').append(Character.toUpperCase(text.charAt(i + 1))).append(Character.toUpperCase(text.charAt(i + 2)));
                }
                i += 2;
            } else {
                out.append(c);
            }
        }
        return out.toString();
    }

    private static String decodeAll(String text) {
        byte[] bytes = new byte[text.length()];
        int count = 0;
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c == '%') {
                bytes[count++] = (byte) (hex(text.charAt(i + 1)) * 16 + hex(text.charAt(i + 2)));
                i += 2;
            } else {
                bytes[count++] = (byte) c;
            }
        }
        return new String(bytes, 0, count, StandardCharsets.UTF_8);
    }

    private static int hex(char c) {
        if (c >= '0' && c <= '9') {
            return c - '0';
        }
        if (c >= 'a' && c <= 'f') {
            return c - 'a' + 10;
        }
        if (c >= 'A' && c <= 'F') {
            return c - 'A' + 10;
        }
        return -1;
    }

    private static boolean isUnreserved(char c) {
        return (c >= 'A' && c <= 'Z')
                || (c >= 'a' && c <= 'z')
                || (c >= '0' && c <= '9')
                || c == '-' || c == '.' || c == '_' || c == '~';
    }

    private record SplitQuery(String main, String query) {
    }

    private record SchemeMatch(String name) {
    }

    private record HostPort(String host, int port) {
    }
}
