package gitignore.pattern.compiler;

import java.util.regex.Pattern;

public final class CompiledPattern {
    private final String source;
    private final boolean isNegation;
    private final boolean isAnchored;
    private final boolean isDirOnly;
    private final String body;
    private final String regex;
    private final Pattern pattern;

    public CompiledPattern(String source, boolean isNegation, boolean isAnchored,
                           boolean isDirOnly, String body, String regex) {
        this.source = source;
        this.isNegation = isNegation;
        this.isAnchored = isAnchored;
        this.isDirOnly = isDirOnly;
        this.body = body;
        this.regex = regex;
        this.pattern = Pattern.compile(regex);
    }

    public String source() { return source; }
    public boolean isNegation() { return isNegation; }
    public boolean isAnchored() { return isAnchored; }
    public boolean isDirOnly() { return isDirOnly; }
    public String body() { return body; }
    public String regex() { return regex; }

    public boolean matches(String path) {
        return pattern.matcher(path).matches();
    }
}
