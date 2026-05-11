package rfc8089.file.uri;

import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;

public final class FileUri {

    private FileUri() {}

    public static boolean isFileUri(String s) {
        if (s == null || s.length() < 5) return false;
        return s.substring(0, 5).equalsIgnoreCase("file:");
    }

    public static boolean looksLikeBarePath(String s) {
        if (s == null || s.isEmpty()) return false;
        int colon = s.indexOf(':');
        if (colon >= 2) {
            char first = s.charAt(0);
            if (isAlpha(first)) {
                boolean validScheme = true;
                for (int i = 1; i < colon; i++) {
                    char c = s.charAt(i);
                    if (!(isAlpha(c) || isDigit(c) || c == '+' || c == '-' || c == '.')) {
                        validScheme = false;
                        break;
                    }
                }
                if (validScheme) return false;
            }
        }
        return true;
    }

    public static String pathToFileUri(String path, String cwd) {
        if (path == null || path.isEmpty()) {
            throw new FileUriException("path is empty", null);
        }
        // UNC: \\server\share\rest
        if (path.startsWith("\\\\")) {
            String rest = path.substring(2).replace('\\', '/');
            int slash = rest.indexOf('/');
            String authority;
            String pathPart;
            if (slash < 0) {
                authority = rest;
                pathPart = "";
            } else {
                authority = rest.substring(0, slash);
                pathPart = rest.substring(slash);
            }
            return "file://" + authority + percentEncodePath(pathPart);
        }
        // Windows DOS-style drive letter
        if (path.length() >= 2 && isAlpha(path.charAt(0)) && path.charAt(1) == ':') {
            if (path.length() == 2) {
                String resolved = ensureAbs(cwd);
                return "file://" + percentEncodePath(resolved);
            }
            char sep = path.charAt(2);
            if (sep == '\\' || sep == '/') {
                String rest = path.substring(3).replace('\\', '/');
                String pathPart = "/" + path.charAt(0) + ":/" + rest;
                return "file://" + percentEncodePath(pathPart);
            }
            // Drive letter without separator: resolve remainder against cwd
            String relPart = path.substring(2);
            String resolved = joinCwd(cwd, relPart);
            return "file://" + percentEncodePath(resolved);
        }
        // POSIX absolute
        if (path.charAt(0) == '/') {
            return "file://" + percentEncodePath(path);
        }
        // POSIX relative
        String resolved = joinCwd(cwd, path);
        return "file://" + percentEncodePath(resolved);
    }

    public static String fileUriToPath(String uri, String style) {
        if (uri == null) throw new FileUriException("uri is null", 0);
        if (!isFileUri(uri)) {
            throw new FileUriException("not a file: URI", 0);
        }
        String rest = uri.substring(5);
        String authority;
        String path;
        int pathStart;

        if (rest.startsWith("//")) {
            String afterAuth = rest.substring(2);
            int slash = afterAuth.indexOf('/');
            if (slash < 0) {
                authority = afterAuth;
                path = "";
                pathStart = uri.length();
            } else {
                authority = afterAuth.substring(0, slash);
                path = afterAuth.substring(slash);
                pathStart = 5 + 2 + slash;
            }
        } else {
            authority = "";
            path = rest;
            pathStart = 5;
        }

        String decoded = percentDecode(path, pathStart);
        boolean windows = "windows".equals(style);

        if (authority.isEmpty() || authority.equalsIgnoreCase("localhost")) {
            // Detect Windows DOS drive: /<letter>: or /<letter>:/<rest>
            if (decoded.length() >= 3 && decoded.charAt(0) == '/'
                    && isAlpha(decoded.charAt(1)) && decoded.charAt(2) == ':') {
                if (decoded.length() == 3 || decoded.charAt(3) == '/') {
                    String drive = decoded.substring(1);
                    return windows ? drive.replace('/', '\\') : drive;
                }
            }
            return windows ? decoded.replace('/', '\\') : decoded;
        }
        // UNC server name
        if (windows) {
            return "\\\\" + authority + decoded.replace('/', '\\');
        }
        return "//" + authority + decoded;
    }

    private static String joinCwd(String cwd, String rel) {
        if (cwd == null) cwd = "";
        cwd = cwd.replace('\\', '/');
        rel = rel.replace('\\', '/');
        if (cwd.isEmpty()) return rel;
        if (cwd.endsWith("/")) return cwd + rel;
        return cwd + "/" + rel;
    }

    private static String ensureAbs(String cwd) {
        if (cwd == null || cwd.isEmpty()) return "/";
        return cwd.replace('\\', '/');
    }

    private static String percentEncodePath(String s) {
        byte[] bytes = s.getBytes(StandardCharsets.UTF_8);
        StringBuilder sb = new StringBuilder(bytes.length);
        for (byte b : bytes) {
            int u = b & 0xFF;
            if (u < 128 && shouldNotEncode((char) u)) {
                sb.append((char) u);
            } else {
                sb.append('%');
                String hex = Integer.toHexString(u).toUpperCase();
                if (hex.length() == 1) sb.append('0');
                sb.append(hex);
            }
        }
        return sb.toString();
    }

    private static boolean shouldNotEncode(char c) {
        return isUnreserved(c) || c == '/' || c == ':';
    }

    private static boolean isUnreserved(char c) {
        return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z')
                || (c >= '0' && c <= '9')
                || c == '-' || c == '_' || c == '.' || c == '~';
    }

    private static String percentDecode(String s, int offsetBase) {
        ByteArrayOutputStream baos = new ByteArrayOutputStream();
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == '%') {
                if (i + 2 >= s.length()) {
                    throw new FileUriException("truncated percent encoding", offsetBase + i);
                }
                int h1 = hex(s.charAt(i + 1));
                int h2 = hex(s.charAt(i + 2));
                if (h1 < 0 || h2 < 0) {
                    throw new FileUriException("invalid percent-encoded octet", offsetBase + i);
                }
                baos.write((h1 << 4) | h2);
                i += 2;
            } else if (c < 128) {
                baos.write(c);
            } else {
                byte[] bytes = String.valueOf(c).getBytes(StandardCharsets.UTF_8);
                for (byte b : bytes) baos.write(b);
            }
        }
        return new String(baos.toByteArray(), StandardCharsets.UTF_8);
    }

    private static int hex(char c) {
        if (c >= '0' && c <= '9') return c - '0';
        if (c >= 'a' && c <= 'f') return c - 'a' + 10;
        if (c >= 'A' && c <= 'F') return c - 'A' + 10;
        return -1;
    }

    private static boolean isAlpha(char c) {
        return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
    }

    private static boolean isDigit(char c) {
        return c >= '0' && c <= '9';
    }
}
