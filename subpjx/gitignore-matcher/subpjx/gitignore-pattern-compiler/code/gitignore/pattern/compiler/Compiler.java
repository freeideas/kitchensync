package gitignore.pattern.compiler;

import java.util.ArrayList;
import java.util.List;

public final class Compiler {

    private Compiler() {}

    public static CompileResult compilePatterns(String text) {
        List<CompiledPattern> patterns = new ArrayList<>();
        List<Diagnostic> diagnostics = new ArrayList<>();
        String[] lines = text.split("\n", -1);
        for (int i = 0; i < lines.length; i++) {
            String raw = lines[i];
            int lineNumber = i + 1;

            int firstNonWs = -1;
            for (int j = 0; j < raw.length(); j++) {
                if (!isWhitespace(raw.charAt(j))) {
                    firstNonWs = j;
                    break;
                }
            }
            if (firstNonWs == -1) continue;
            if (raw.charAt(firstNonWs) == '#') continue;

            String source = stripTrailingWhitespace(raw);

            boolean isNegation = false;
            boolean isAnchored = false;
            boolean isDirOnly = false;
            String body = source;

            if (!body.isEmpty() && body.charAt(0) == '!') {
                isNegation = true;
                body = body.substring(1);
            }
            if (!body.isEmpty() && body.charAt(0) == '/') {
                isAnchored = true;
                body = body.substring(1);
            }
            if (body.length() >= 1 && body.charAt(body.length() - 1) == '/'
                    && !(body.length() >= 2 && body.charAt(body.length() - 2) == '\\')) {
                isDirOnly = true;
                body = body.substring(0, body.length() - 1);
            }

            try {
                String regex = compileBody(body);
                patterns.add(new CompiledPattern(source, isNegation, isAnchored, isDirOnly, body, regex));
            } catch (CompileException ex) {
                diagnostics.add(new Diagnostic(lineNumber, raw, ex.getMessage()));
            }
        }
        return new CompileResult(new PatternSet(patterns), diagnostics);
    }

    private static boolean isWhitespace(char c) {
        return c == ' ' || c == '\t' || c == '\r';
    }

    private static String stripTrailingWhitespace(String s) {
        int i = s.length() - 1;
        while (i >= 0 && isWhitespace(s.charAt(i))) {
            if (i > 0 && s.charAt(i - 1) == '\\') {
                break;
            }
            i--;
        }
        return s.substring(0, i + 1);
    }

    public static String compileBody(String body) throws CompileException {
        StringBuilder sb = new StringBuilder();
        sb.append("^");
        int n = body.length();
        int i = 0;
        while (i < n) {
            char c = body.charAt(i);
            if (c == '*') {
                if (i + 1 < n && body.charAt(i + 1) == '*') {
                    boolean leading = (i == 0 || body.charAt(i - 1) == '/');
                    boolean trailing = (i + 2 == n || body.charAt(i + 2) == '/');
                    if (leading && trailing) {
                        if (i == 0 && i + 2 == n) {
                            sb.append(".*");
                            i += 2;
                        } else if (i == 0) {
                            sb.append("(?:.*/)?");
                            i += 3;
                        } else if (i + 2 == n) {
                            sb.append(".+");
                            i += 2;
                        } else {
                            sb.append("(?:.*/)?");
                            i += 3;
                        }
                    } else {
                        sb.append("[^/]*");
                        i += 2;
                    }
                } else {
                    sb.append("[^/]*");
                    i += 1;
                }
            } else if (c == '?') {
                sb.append("[^/]");
                i += 1;
            } else if (c == '[') {
                int j = i + 1;
                StringBuilder cls = new StringBuilder();
                cls.append("[");
                if (j < n && (body.charAt(j) == '!' || body.charAt(j) == '^')) {
                    cls.append("^");
                    j++;
                }
                boolean first = true;
                while (j < n) {
                    char cc = body.charAt(j);
                    if (cc == ']' && !first) {
                        break;
                    }
                    if (cc == '\\' && j + 1 < n) {
                        char next = body.charAt(j + 1);
                        cls.append('\\').append(next);
                        j += 2;
                    } else if (cc == ']' && first) {
                        cls.append("\\]");
                        j++;
                    } else if (cc == '\\' || cc == '^') {
                        cls.append('\\').append(cc);
                        j++;
                    } else {
                        cls.append(cc);
                        j++;
                    }
                    first = false;
                }
                if (j >= n) {
                    throw new CompileException("unclosed character class");
                }
                cls.append("]");
                sb.append(cls);
                i = j + 1;
            } else if (c == '\\') {
                if (i + 1 >= n) {
                    throw new CompileException("trailing backslash with nothing to escape");
                }
                char next = body.charAt(i + 1);
                if (isRegexMeta(next)) {
                    sb.append('\\').append(next);
                } else {
                    sb.append(next);
                }
                i += 2;
            } else {
                if (isRegexMeta(c)) {
                    sb.append('\\').append(c);
                } else {
                    sb.append(c);
                }
                i += 1;
            }
        }
        sb.append("$");
        return sb.toString();
    }

    private static boolean isRegexMeta(char c) {
        return "\\.+*?()|[]{}^$".indexOf(c) >= 0;
    }
}
