package gitignore.scope.stack.matcher;

import java.util.regex.Pattern;

/** Converts a gitignore-style glob body to a Java regex and matches against text. */
final class Glob {
    private Glob() {}

    /** Match the entire `text` against the glob `body`, where `*` does not cross `/` and `**` does. */
    static boolean matches(String body, String text) {
        return Pattern.compile("^" + toRegex(body) + "$").matcher(text).matches();
    }

    private static String toRegex(String body) {
        StringBuilder out = new StringBuilder();
        int n = body.length();
        for (int i = 0; i < n; i++) {
            char c = body.charAt(i);
            if (c == '*') {
                if (i + 1 < n && body.charAt(i + 1) == '*') {
                    out.append(".*");
                    i++;
                } else {
                    out.append("[^/]*");
                }
            } else if (c == '?') {
                out.append("[^/]");
            } else if (c == '[') {
                int j = i + 1;
                if (j < n && (body.charAt(j) == '!' || body.charAt(j) == '^')) j++;
                if (j < n && body.charAt(j) == ']') j++;
                while (j < n && body.charAt(j) != ']') j++;
                if (j < n) {
                    String cls = body.substring(i + 1, j);
                    if (cls.startsWith("!")) cls = "^" + cls.substring(1);
                    out.append('[').append(cls).append(']');
                    i = j;
                } else {
                    out.append(Pattern.quote(String.valueOf(c)));
                }
            } else {
                if ("\\.+(){}|^$".indexOf(c) >= 0) {
                    out.append('\\').append(c);
                } else {
                    out.append(c);
                }
            }
        }
        return out.toString();
    }
}
