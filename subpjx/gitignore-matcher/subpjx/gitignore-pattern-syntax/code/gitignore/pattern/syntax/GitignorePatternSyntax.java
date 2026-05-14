package gitignore.pattern.syntax;

import java.util.ArrayList;
import java.util.List;
import java.util.regex.Pattern;
import java.util.regex.PatternSyntaxException;

public final class GitignorePatternSyntax {
    private static final String INVALID_PATTERN = "invalid_pattern";
    private static final String INVALID_PATH = "invalid_path";

    public static final class PatternSyntaxException extends RuntimeException {
        private final String code;
        private final String detail;

        PatternSyntaxException(String code, String detail) {
            super(code + ": " + detail);
            this.code = code;
            this.detail = detail;
        }

        public String code() {
            return code;
        }

        public String detail() {
            return detail;
        }
    }

    private GitignorePatternSyntax() {}

    public static PatternRule[] compile_patterns(PatternLine[] patternLines) {
        if (patternLines == null) {
            throw new PatternSyntaxException(INVALID_PATTERN, "pattern_lines is required");
        }

        List<PatternRule> compiled = new ArrayList<>();
        for (int i = 0; i < patternLines.length; i++) {
            PatternLine line = patternLines[i];
            String text = line == null ? null : line.text();
            if (text == null) {
                throw new PatternSyntaxException(INVALID_PATTERN, "pattern_lines[" + i + "].text is required");
            }

            String trimmed = text.strip();
            if (trimmed.isEmpty() || trimmed.startsWith("#")) {
                continue;
            }

            boolean negated = false;
            if (trimmed.startsWith("!")) {
                negated = true;
                trimmed = trimmed.substring(1);
            }
            if (trimmed.isEmpty()) {
                throw new PatternSyntaxException(INVALID_PATTERN, "empty pattern at index " + i);
            }

            boolean anchored = false;
            if (trimmed.startsWith("/")) {
                anchored = true;
                trimmed = trimmed.substring(1);
            }

            boolean directoryOnly = false;
            if (trimmed.endsWith("/")) {
                directoryOnly = true;
                trimmed = trimmed.substring(0, trimmed.length() - 1);
            }
            if (trimmed.isEmpty()) {
                throw new PatternSyntaxException(INVALID_PATTERN, "empty pattern at index " + i);
            }

            validatePatternText(trimmed, i);

            boolean hasSlash = trimmed.contains("/");
            String regex = compilePatternToRegex(trimmed, anchored, hasSlash);

            compiled.add(new PatternRule(
                text,
                negated,
                directoryOnly,
                anchored,
                hasSlash,
                regex
            ));
        }

        return compiled.toArray(new PatternRule[0]);
    }

    public static PatternRule[] compilePatterns(PatternLine[] patternLines) {
        return compile_patterns(patternLines);
    }

    public static PatternMatchResult match_patterns(PatternRule[] patternRules, PatternMatchInput input) {
        if (patternRules == null) {
            throw new PatternSyntaxException(INVALID_PATTERN, "pattern_rules is required");
        }
        if (input == null) {
            throw new PatternSyntaxException(INVALID_PATH, "input is required");
        }

        String path = input.path();
        validatePath(path);

        boolean matched = false;
        String status = PatternMatchResult.INCLUDED;

        for (PatternRule rule : patternRules) {
            if (rule == null) {
                throw new PatternSyntaxException(INVALID_PATTERN, "pattern_rules contains null rule");
            }

            Pattern matcher = rulePattern(rule);
            boolean doesMatch = matcher.matcher(path).matches();
            if (!doesMatch) {
                continue;
            }
            if (rule.directoryOnly() && !input.is_directory()) {
                continue;
            }

            matched = true;
            status = rule.negated() ? PatternMatchResult.INCLUDED : PatternMatchResult.IGNORED;
        }

        return new PatternMatchResult(matched, status);
    }

    public static PatternMatchResult matchPatterns(PatternRule[] patternRules, PatternMatchInput input) {
        return match_patterns(patternRules, input);
    }

    private static void validatePatternText(String pattern, int index) {
        if (pattern.indexOf('\u0000') >= 0) {
            throw new PatternSyntaxException(INVALID_PATTERN, "null character at index " + index);
        }
        if (pattern.contains("[") && !pattern.contains("]")) {
            throw new PatternSyntaxException(INVALID_PATTERN, "unterminated character class at index " + index);
        }
    }

    private static void validatePath(String path) {
        if (path == null || path.isBlank()) {
            throw new PatternSyntaxException(INVALID_PATH, "path is required");
        }
        if (path.contains("\\")) {
            throw new PatternSyntaxException(INVALID_PATH, "backslash is not supported: " + path);
        }
        if (path.startsWith("/")) {
            throw new PatternSyntaxException(INVALID_PATH, "absolute path is not allowed: " + path);
        }
        if (path.endsWith("/")) {
            throw new PatternSyntaxException(INVALID_PATH, "path must be a relative path segment: " + path);
        }

        String[] segments = path.split("/", -1);
        for (String segment : segments) {
            if (segment.isEmpty()) {
                throw new PatternSyntaxException(INVALID_PATH, "invalid path segment: " + path);
            }
            if (segment.equals(".") || segment.equals("..")) {
                throw new PatternSyntaxException(INVALID_PATH, "path traversal is not allowed: " + path);
            }
        }
    }

    private static String normalizeRulePatternForCompile(String pattern) {
        if (pattern == null) {
            throw new PatternSyntaxException(INVALID_PATTERN, "pattern is required");
        }
        String normalized = pattern.strip();
        if (normalized.startsWith("!")) {
            normalized = normalized.substring(1);
        }
        if (normalized.startsWith("/")) {
            normalized = normalized.substring(1);
        }
        if (normalized.endsWith("/")) {
            normalized = normalized.substring(0, normalized.length() - 1);
        }
        if (normalized.isBlank()) {
            throw new PatternSyntaxException(INVALID_PATTERN, "pattern is empty");
        }
        return normalized;
    }
    
    private static String compilePatternToRegex(String pattern, boolean anchored, boolean hasSlash) {
        String translated = globToRegex(pattern);
        String regex;
        if (anchored) {
            regex = "^" + translated + "$";
        } else if (hasSlash) {
            regex = "^(?:.*/)?" + translated + "$";
        } else {
            regex = "^(?:.*/)?" + translated + "$";
        }
        return regex;
    }

    private static Pattern rulePattern(PatternRule rule) {
        if (rule == null) {
            throw new PatternSyntaxException(INVALID_PATTERN, "pattern rule is required");
        }
        String regex = rule.regex();
        if (regex == null || regex.isBlank()) {
            regex = compilePatternToRegex(
                    normalizeRulePatternForCompile(rule.pattern()),
                    rule.anchored(),
                    rule.hasSlash()
            );
        }
        try {
            return Pattern.compile(regex);
        } catch (PatternSyntaxException ex) {
            throw new PatternSyntaxException(INVALID_PATTERN, "cannot compile pattern regex");
        }
    }

    private static String globToRegex(String pattern) {
        StringBuilder out = new StringBuilder();
        for (int i = 0; i < pattern.length(); i++) {
            char c = pattern.charAt(i);
            if (c == '*') {
                if (i + 1 < pattern.length() && pattern.charAt(i + 1) == '*') {
                    out.append(".*");
                    i++;
                    continue;
                }
                out.append("[^/]*");
                continue;
            }
            if (c == '?') {
                out.append("[^/]");
                continue;
            }
            if (c == '.') {
                out.append("\\.");
                continue;
            }
            if (c == '(' || c == ')' || c == '[' || c == ']' || c == '{' || c == '}' || c == '^' || c == '$'
                    || c == '+' || c == '|' || c == '\\') {
                out.append('\\').append(c);
                continue;
            }
            out.append(c);
        }
        return out.toString();
    }
}
