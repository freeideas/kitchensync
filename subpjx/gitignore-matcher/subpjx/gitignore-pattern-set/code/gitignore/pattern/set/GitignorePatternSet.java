package gitignore.pattern.set;

import java.util.ArrayList;
import java.util.List;
import java.util.Objects;
import java.util.regex.Pattern;
import java.util.regex.PatternSyntaxException;

public final class GitignorePatternSet {
    private final List<Rule> rules;

    private GitignorePatternSet(List<Rule> rules) {
        this.rules = List.copyOf(rules);
    }

    public static GitignorePatternSet empty() {
        return new GitignorePatternSet(List.of());
    }

    public static GitignorePatternSet compile(PatternSetSource source) {
        Objects.requireNonNull(source, "source");
        String text = Objects.requireNonNull(source.pattern_text(), "pattern_text");
        if (text.indexOf('\0') >= 0) {
            throw new GitignorePatternSetException(ErrorCategory.invalid_pattern_text, "pattern text contains NUL");
        }

        List<Rule> compiled = new ArrayList<>();
        int lineNumber = 1;
        int start = 0;
        for (int i = 0; i <= text.length(); i++) {
            boolean atEnd = i == text.length();
            char ch = atEnd ? '\0' : text.charAt(i);
            if (atEnd || ch == '\n' || ch == '\r') {
                String line = text.substring(start, i);
                Rule rule = parseLine(line, lineNumber, source.source_name());
                if (rule != null) {
                    compiled.add(rule);
                }
                if (!atEnd && ch == '\r' && i + 1 < text.length() && text.charAt(i + 1) == '\n') {
                    i++;
                }
                start = i + 1;
                lineNumber++;
            }
        }
        return new GitignorePatternSet(compiled);
    }

    public PatternMatch match(PathEntry entry) {
        Objects.requireNonNull(entry, "entry");
        validatePath(entry.relative_path());
        Objects.requireNonNull(entry.kind(), "kind");

        Rule finalRule = null;
        for (Rule rule : rules) {
            if (rule.matches(entry)) {
                finalRule = rule;
            }
        }
        if (finalRule == null) {
            return PatternMatch.none();
        }
        PatternDecision decision = finalRule.negated ? PatternDecision.include : PatternDecision.ignore;
        return new PatternMatch(decision, finalRule.negated, finalRule.sourceName, finalRule.lineNumber, finalRule.originalPattern);
    }

    private static Rule parseLine(String rawLine, int lineNumber, String sourceName) {
        String line = trimUnescapedTrailingSpaces(rawLine);
        if (line.isEmpty() || line.charAt(0) == '#') {
            return null;
        }

        String originalPattern = line;
        boolean negated = false;
        if (line.charAt(0) == '!') {
            negated = true;
            line = line.substring(1);
        }
        if (line.isEmpty()) {
            return null;
        }

        boolean directoryOnly = line.endsWith("/");
        String body = directoryOnly ? line.substring(0, line.length() - 1) : line;
        boolean rootRelative = body.startsWith("/");
        if (rootRelative) {
            body = body.substring(1);
        }
        if (body.isEmpty()) {
            return null;
        }

        boolean hasSlash = body.indexOf('/') >= 0;
        Pattern regex = Pattern.compile("^" + globToRegex(body) + "$");
        return new Rule(lineNumber, sourceName, negated, directoryOnly, rootRelative || hasSlash, originalPattern, regex);
    }

    private static void validatePath(String path) {
        if (path == null || path.isEmpty()) {
            throw new GitignorePatternSetException(ErrorCategory.invalid_path, "path is empty");
        }
        if (path.charAt(0) == '/') {
            throw new GitignorePatternSetException(ErrorCategory.invalid_path, "path starts with slash");
        }
        if (path.charAt(path.length() - 1) == '/') {
            throw new GitignorePatternSetException(ErrorCategory.invalid_path, "path ends with slash");
        }
        if (path.indexOf('\\') >= 0) {
            throw new GitignorePatternSetException(ErrorCategory.invalid_path, "path contains backslash");
        }
        if (path.indexOf('\0') >= 0) {
            throw new GitignorePatternSetException(ErrorCategory.invalid_path, "path contains NUL");
        }
        String[] segments = path.split("/", -1);
        for (String segment : segments) {
            if (segment.isEmpty()) {
                throw new GitignorePatternSetException(ErrorCategory.invalid_path, "path contains empty segment");
            }
            if (segment.equals(".") || segment.equals("..")) {
                throw new GitignorePatternSetException(ErrorCategory.invalid_path, "path contains dot segment");
            }
        }
    }

    private static String trimUnescapedTrailingSpaces(String line) {
        int end = line.length();
        while (end > 0 && line.charAt(end - 1) == ' ' && !isEscaped(line, end - 1)) {
            end--;
        }
        return line.substring(0, end);
    }

    private static boolean isEscaped(String text, int index) {
        int slashCount = 0;
        for (int i = index - 1; i >= 0 && text.charAt(i) == '\\'; i--) {
            slashCount++;
        }
        return slashCount % 2 == 1;
    }

    private static String globToRegex(String glob) {
        StringBuilder out = new StringBuilder();
        for (int i = 0; i < glob.length(); ) {
            if (i == 0 && glob.startsWith("**/", i)) {
                out.append("(?:[^/]+/)*");
                i += 3;
            } else if (glob.startsWith("/**/", i)) {
                out.append("/(?:[^/]+/)*");
                i += 4;
            } else if (glob.startsWith("/**", i) && i + 3 == glob.length()) {
                out.append("/.+");
                i += 3;
            } else {
                char ch = glob.charAt(i);
                if (ch == '*') {
                    out.append("[^/]*");
                    i++;
                } else if (ch == '?') {
                    out.append("[^/]");
                    i++;
                } else if (ch == '[') {
                    Bracket bracket = parseBracket(glob, i);
                    out.append(bracket.regex);
                    i = bracket.nextIndex;
                } else if (ch == '\\') {
                    if (i + 1 < glob.length()) {
                        appendLiteral(out, glob.charAt(i + 1));
                        i += 2;
                    } else {
                        appendLiteral(out, ch);
                        i++;
                    }
                } else {
                    appendLiteral(out, ch);
                    i++;
                }
            }
        }
        return out.toString();
    }

    private static Bracket parseBracket(String glob, int start) {
        int end = glob.indexOf(']', start + 1);
        if (end < 0) {
            return new Bracket("\\[", start + 1);
        }
        String content = glob.substring(start + 1, end);
        if (content.isEmpty() || content.equals("!")) {
            return literalBracket(glob, start, end);
        }
        String regex = bracketRegex(content);
        try {
            Pattern.compile(regex);
            return new Bracket(regex, end + 1);
        } catch (PatternSyntaxException ex) {
            return literalBracket(glob, start, end);
        }
    }

    private static Bracket literalBracket(String glob, int start, int end) {
        StringBuilder literal = new StringBuilder();
        for (int i = start; i <= end; i++) {
            appendLiteral(literal, glob.charAt(i));
        }
        return new Bracket(literal.toString(), end + 1);
    }

    private static String bracketRegex(String content) {
        StringBuilder out = new StringBuilder("(?=[^/])[");
        int start = 0;
        if (content.charAt(0) == '!') {
            out.append('^');
            start = 1;
        }
        for (int i = start; i < content.length(); i++) {
            char ch = content.charAt(i);
            if (ch == '\\' || ch == '[' || ch == '&') {
                out.append('\\');
            }
            if (ch == '^' && i == start) {
                out.append('\\');
            }
            out.append(ch);
        }
        out.append(']');
        return out.toString();
    }

    private static void appendLiteral(StringBuilder out, char ch) {
        if ("\\.[]{}()+-^$|?".indexOf(ch) >= 0) {
            out.append('\\');
        }
        out.append(ch);
    }

    private record Bracket(String regex, int nextIndex) {
    }

    private static final class Rule {
        private final int lineNumber;
        private final String sourceName;
        private final boolean negated;
        private final boolean directoryOnly;
        private final boolean rootRelative;
        private final String originalPattern;
        private final Pattern regex;

        private Rule(int lineNumber, String sourceName, boolean negated, boolean directoryOnly,
                boolean rootRelative, String originalPattern, Pattern regex) {
            this.lineNumber = lineNumber;
            this.sourceName = sourceName;
            this.negated = negated;
            this.directoryOnly = directoryOnly;
            this.rootRelative = rootRelative;
            this.originalPattern = originalPattern;
            this.regex = regex;
        }

        private boolean matches(PathEntry entry) {
            if (!directoryOnly) {
                return matchesPath(entry.relative_path());
            }
            String path = entry.relative_path();
            int end = -1;
            while (true) {
                end = path.indexOf('/', end + 1);
                String prefix = end < 0 ? path : path.substring(0, end);
                boolean completePath = end < 0;
                if ((!completePath || entry.kind() == EntryKind.directory) && matchesPath(prefix)) {
                    return true;
                }
                if (completePath) {
                    return false;
                }
            }
        }

        private boolean matchesPath(String path) {
            if (rootRelative) {
                return regex.matcher(path).matches();
            }
            int start = 0;
            while (start <= path.length()) {
                int slash = path.indexOf('/', start);
                String segment = slash < 0 ? path.substring(start) : path.substring(start, slash);
                if (regex.matcher(segment).matches()) {
                    return true;
                }
                if (slash < 0) {
                    return false;
                }
                start = slash + 1;
            }
            return false;
        }
    }
}
