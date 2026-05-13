package url.parser;

import java.util.*;

public final class UrlParser {

    private static final Set<String> ALLOWED_PARAMS = Set.of("mc", "ct", "ka");

    public static TaggedGroup parse(String text, String cwd, String defaultUser) {
        if (text == null || text.isEmpty())
            throw new ParseException("empty input");

        Role role = Role.NORMAL;
        String rest = text;

        if (rest.startsWith("+")) {
            role = Role.CANON;
            rest = rest.substring(1);
        } else if (rest.startsWith("-")) {
            role = Role.SUBORDINATE;
            rest = rest.substring(1);
        }

        if (rest.isEmpty())
            throw new ParseException("empty input");

        if (role != Role.NORMAL && !rest.isEmpty()
                && (rest.charAt(0) == '+' || rest.charAt(0) == '-'))
            throw new ParseException("multiple role tags");

        List<ParsedUrl> urls;
        if (rest.startsWith("[")) {
            urls = parseBracketGroup(rest, cwd, defaultUser);
        } else {
            urls = List.of(parseSingleUrl(rest, cwd, defaultUser, false));
        }

        return new TaggedGroup(role, urls);
    }

    public static String normalize(String url, String cwd, String defaultUser) {
        return parse(url, cwd, defaultUser).urls().get(0).identity();
    }

    private static List<ParsedUrl> parseBracketGroup(String text, String cwd, String defaultUser) {
        if (!text.endsWith("]"))
            throw new ParseException("bracket group not closed");
        String inner = text.substring(1, text.length() - 1);
        String[] parts = inner.split(",", -1);
        List<ParsedUrl> result = new ArrayList<>();
        for (String part : parts) {
            if (part.isEmpty())
                throw new ParseException("bracket group contains empty URL");
            result.add(parseSingleUrl(part, cwd, defaultUser, true));
        }
        return Collections.unmodifiableList(result);
    }

    private static ParsedUrl parseSingleUrl(String text, String cwd, String defaultUser,
                                             boolean inBracket) {
        if (inBracket && !text.isEmpty()
                && (text.charAt(0) == '+' || text.charAt(0) == '-'))
            throw new ParseException("role tag on inner URL in bracket group");

        String lower = text.toLowerCase(Locale.ROOT);
        if (lower.startsWith("file://")) return parseFileUri(text, cwd);
        if (lower.startsWith("sftp://")) return parseSftpUri(text, defaultUser);
        if (isBarePath(text)) return parseBarePath(text, cwd);
        if (text.contains("://")) {
            String scheme = text.substring(0, text.indexOf("://"));
            throw new ParseException("unrecognized scheme: " + scheme);
        }
        throw new ParseException("unrecognized URL format: " + text);
    }

    private static boolean isBarePath(String text) {
        if (text.isEmpty()) return false;
        char c0 = text.charAt(0);
        if (c0 == '/' || c0 == '\\') return true;
        if (text.startsWith("./") || text.startsWith(".\\")
                || text.startsWith("../") || text.startsWith("..\\" )
                || text.equals(".") || text.equals("..")) return true;
        if (text.length() >= 2 && Character.isLetter(c0) && text.charAt(1) == ':') return true;
        return false;
    }

    private static ParsedUrl parseFileUri(String text, String cwd) {
        String rest = text.substring("file://".length());

        String queryStr = null;
        int q = rest.indexOf('?');
        if (q >= 0) { queryStr = rest.substring(q + 1); rest = rest.substring(0, q); }

        String rawPath;
        if (rest.startsWith("/")) {
            rawPath = rest;
        } else {
            int slash = rest.indexOf('/');
            rawPath = (slash >= 0) ? rest.substring(slash) : "/";
        }

        String path = normalizePath(rawPath);
        Map<String, String> query = parseQueryString(queryStr);
        String identity = "file://" + percentDecodeUnreserved(path);
        return new ParsedUrl("file", null, null, null, null, path, query, identity);
    }

    private static ParsedUrl parseSftpUri(String text, String defaultUser) {
        String rest = text.substring("sftp://".length());

        String queryStr = null;
        int q = rest.indexOf('?');
        if (q >= 0) { queryStr = rest.substring(q + 1); rest = rest.substring(0, q); }

        int slash = rest.indexOf('/');
        String authority = (slash >= 0) ? rest.substring(0, slash) : rest;
        String rawPath = (slash >= 0) ? rest.substring(slash) : "/";

        String user = null, password = null;
        int at = authority.lastIndexOf('@');
        String hostPort;
        if (at >= 0) {
            String userinfo = authority.substring(0, at);
            hostPort = authority.substring(at + 1);
            int colon = userinfo.indexOf(':');
            if (colon >= 0) {
                user = userinfo.substring(0, colon);
                password = userinfo.substring(colon + 1);
            } else {
                user = userinfo;
            }
        } else {
            hostPort = authority;
        }

        String host;
        Integer port = null;
        if (hostPort.startsWith("[")) {
            int close = hostPort.indexOf(']');
            if (close < 0) throw new ParseException("invalid IPv6 address in sftp URL");
            host = hostPort.substring(1, close);
            if (close + 1 < hostPort.length() && hostPort.charAt(close + 1) == ':')
                port = parsePort(hostPort.substring(close + 2));
        } else {
            int colon = hostPort.lastIndexOf(':');
            if (colon >= 0) {
                host = hostPort.substring(0, colon);
                port = parsePort(hostPort.substring(colon + 1));
            } else {
                host = hostPort;
            }
        }

        if (host.isEmpty()) throw new ParseException("sftp URL without a host");
        if (port != null && (port < 1 || port > 65535))
            throw new ParseException("sftp port out of range: " + port);

        String path = normalizePath(rawPath);
        Map<String, String> query = parseQueryString(queryStr);
        String identity = buildSftpIdentity(user, password, host, port, path, defaultUser);
        return new ParsedUrl("sftp", user, password, host, port, path, query, identity);
    }

    private static ParsedUrl parseBarePath(String text, String cwd) {
        String pathText = text;
        String queryStr = null;
        int q = text.indexOf('?');
        if (q >= 0) { queryStr = text.substring(q + 1); pathText = text.substring(0, q); }

        String path = pathText.replace('\\', '/');

        if (path.length() >= 2 && Character.isLetter(path.charAt(0)) && path.charAt(1) == ':') {
            path = "/" + path;
        }

        if (!path.startsWith("/")) {
            path = cwd + "/" + path;
        }

        path = normalizePath(path);
        Map<String, String> query = parseQueryString(queryStr);
        String identity = "file://" + percentDecodeUnreserved(path);
        return new ParsedUrl("file", null, null, null, null, path, query, identity);
    }

    private static String normalizePath(String path) {
        String[] parts = path.split("/", -1);
        Deque<String> stack = new ArrayDeque<>();
        for (String part : parts) {
            if (part.isEmpty() || part.equals(".")) continue;
            if (part.equals("..")) { if (!stack.isEmpty()) stack.pollLast(); }
            else stack.addLast(part);
        }
        if (stack.isEmpty()) return "/";
        StringBuilder sb = new StringBuilder();
        for (String p : stack) sb.append('/').append(p);
        return sb.toString();
    }

    private static String buildSftpIdentity(String user, String password, String host,
                                              Integer port, String path, String defaultUser) {
        StringBuilder sb = new StringBuilder("sftp://");
        sb.append(user != null ? user : defaultUser);
        if (password != null) sb.append(':').append(password);
        sb.append('@').append(host.toLowerCase(Locale.ROOT));
        if (port != null && port != 22) sb.append(':').append(port);
        sb.append(percentDecodeUnreserved(path));
        return sb.toString();
    }

    private static String percentDecodeUnreserved(String input) {
        if (!input.contains("%")) return input;
        StringBuilder sb = new StringBuilder(input.length());
        int i = 0;
        while (i < input.length()) {
            char c = input.charAt(i);
            if (c == '%' && i + 2 < input.length()) {
                char h1 = input.charAt(i + 1), h2 = input.charAt(i + 2);
                if (isHexDigit(h1) && isHexDigit(h2)) {
                    int val = Character.digit(h1, 16) * 16 + Character.digit(h2, 16);
                    if (isUnreserved((char) val)) { sb.append((char) val); i += 3; continue; }
                }
            }
            sb.append(c);
            i++;
        }
        return sb.toString();
    }

    private static boolean isHexDigit(char c) {
        return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
    }

    private static boolean isUnreserved(char c) {
        return Character.isLetterOrDigit(c) || c == '-' || c == '.' || c == '_' || c == '~';
    }

    private static Map<String, String> parseQueryString(String queryStr) {
        if (queryStr == null || queryStr.isEmpty()) return Map.of();
        Map<String, String> result = new LinkedHashMap<>();
        for (String pair : queryStr.split("&", -1)) {
            if (pair.isEmpty()) continue;
            int eq = pair.indexOf('=');
            String key = (eq >= 0) ? pair.substring(0, eq) : pair;
            String val = (eq >= 0) ? pair.substring(eq + 1) : "";
            if (!ALLOWED_PARAMS.contains(key))
                throw new ParseException("unrecognized query parameter: " + key);
            try {
                int n = Integer.parseInt(val);
                if (n <= 0) throw new ParseException(
                        "query parameter " + key + " must be a positive integer");
            } catch (NumberFormatException e) {
                throw new ParseException(
                        "query parameter " + key + " must be a positive integer");
            }
            result.put(key, val);
        }
        return Collections.unmodifiableMap(result);
    }

    private static int parsePort(String portStr) {
        try { return Integer.parseInt(portStr); }
        catch (NumberFormatException e) { throw new ParseException("invalid port: " + portStr); }
    }
}
