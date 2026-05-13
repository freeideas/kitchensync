package gitignore.matcher;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.regex.Pattern;

public final class GitignoreMatcher {

    private GitignoreMatcher() {}

    public static Patterns compile(String text) {
        List<ParsedPattern> patterns = new ArrayList<>();
        if (text == null || text.isEmpty()) return new Patterns(Collections.unmodifiableList(patterns));
        for (String rawLine : text.split("\n", -1)) {
            String line = stripTrailingWhitespace(rawLine);
            if (line.isEmpty()) continue;
            if (line.charAt(0) == '#') continue;
            patterns.add(parseLine(line));
        }
        return new Patterns(Collections.unmodifiableList(patterns));
    }

    public static MatchResult match(List<StackEntry> stack, String relativePath, boolean isDirectory) {
        if (stack.isEmpty()) return MatchResult.NOT_IGNORED;

        // Parent-directory restriction: if any strict ancestor directory is Ignored, so is this path.
        String[] parts = relativePath.split("/", -1);
        for (int depth = 1; depth < parts.length; depth++) {
            String ancestor = String.join("/", Arrays.copyOfRange(parts, 0, depth));
            List<StackEntry> restricted = new ArrayList<>();
            for (StackEntry e : stack) {
                if (isScopeLE(e.scope(), ancestor)) restricted.add(e);
            }
            if (match(restricted, ancestor, true) == MatchResult.IGNORED) {
                return MatchResult.IGNORED;
            }
        }

        // Core: last matching pattern across the ordered stack wins.
        MatchResult verdict = MatchResult.NOT_IGNORED;
        for (StackEntry entry : stack) {
            String stripped = stripScope(entry.scope(), relativePath);
            if (stripped == null) continue;
            for (ParsedPattern p : entry.patterns().list()) {
                if (p.directoryOnly() && !isDirectory) continue;
                if (p.regex().matcher(stripped).matches()) {
                    verdict = p.negated() ? MatchResult.NOT_IGNORED : MatchResult.IGNORED;
                }
            }
        }
        return verdict;
    }

    // --- parsing helpers ---

    private static String stripTrailingWhitespace(String line) {
        int i = line.length() - 1;
        while (i >= 0 && Character.isWhitespace(line.charAt(i))) i--;
        // If the last non-whitespace char is '\', it escapes the immediately following whitespace.
        if (i >= 0 && line.charAt(i) == '\\' && i + 1 < line.length()) {
            return line.substring(0, i) + ' ';
        }
        return line.substring(0, i + 1);
    }

    private static ParsedPattern parseLine(String line) {
        boolean negated = false;
        if (line.length() >= 2 && line.charAt(0) == '\\' && (line.charAt(1) == '#' || line.charAt(1) == '!')) {
            line = line.substring(1);
        } else if (line.charAt(0) == '!') {
            negated = true;
            line = line.substring(1);
        }

        boolean directoryOnly = line.endsWith("/");
        if (directoryOnly) line = line.substring(0, line.length() - 1);

        boolean hasLeadingSlash = !line.isEmpty() && line.charAt(0) == '/';
        if (hasLeadingSlash) line = line.substring(1);

        boolean anchored = hasLeadingSlash || line.contains("/");
        String regexStr = buildRegex(line, !anchored);
        Pattern regex = Pattern.compile(regexStr);

        return new ParsedPattern(negated, directoryOnly, regex);
    }

    private static String buildRegex(String glob, boolean floating) {
        String pattern = floating ? "**/" + glob : glob;
        StringBuilder sb = new StringBuilder("^");
        int len = pattern.length();
        int i = 0;
        while (i < len) {
            char c = pattern.charAt(i);
            if (c == '*' && i + 1 < len && pattern.charAt(i + 1) == '*') {
                boolean prevSlash = (i == 0 || pattern.charAt(i - 1) == '/');
                boolean nextSlash = (i + 2 < len && pattern.charAt(i + 2) == '/');
                if (prevSlash && nextSlash) {
                    sb.append("(.*/)?");
                    i += 3;
                } else {
                    // trailing ** or standalone **
                    sb.append(".*");
                    i += 2;
                }
            } else if (c == '*') {
                sb.append("[^/]*");
                i++;
            } else if (c == '?') {
                sb.append("[^/]");
                i++;
            } else if (c == '[') {
                int j = i + 1;
                boolean negate = (j < len && pattern.charAt(j) == '!');
                if (negate) j++;
                sb.append('[');
                if (negate) sb.append('^');
                while (j < len && pattern.charAt(j) != ']') {
                    char ch = pattern.charAt(j);
                    if (ch == '\\' && j + 1 < len) {
                        sb.append('\\').append(pattern.charAt(j + 1));
                        j += 2;
                    } else {
                        sb.append(ch);
                        j++;
                    }
                }
                sb.append(']');
                i = j + 1;
            } else if (c == '\\' && i + 1 < len) {
                char next = pattern.charAt(i + 1);
                if (".+^${}()|\\*?[]".indexOf(next) >= 0) {
                    sb.append('\\').append(next);
                } else {
                    sb.append(next);
                }
                i += 2;
            } else if (".+^${}()|".indexOf(c) >= 0) {
                sb.append('\\').append(c);
                i++;
            } else {
                sb.append(c);
                i++;
            }
        }
        sb.append('$');
        return sb.toString();
    }

    // --- scope helpers ---

    private static String stripScope(String scope, String path) {
        if (scope.isEmpty()) return path;
        String prefix = scope + "/";
        if (path.startsWith(prefix)) return path.substring(prefix.length());
        return null;
    }

    private static boolean isScopeLE(String scope, String path) {
        if (scope.isEmpty()) return true;
        return path.equals(scope) || path.startsWith(scope + "/");
    }
}
