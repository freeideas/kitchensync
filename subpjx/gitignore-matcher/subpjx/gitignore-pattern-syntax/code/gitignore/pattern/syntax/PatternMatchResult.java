package gitignore.pattern.syntax;

public record PatternMatchResult(boolean matches, String status) {

    public static final String INCLUDED = "included";
    public static final String IGNORED = "ignored";

    public PatternMatchResult {
        if (!INCLUDED.equals(status) && !IGNORED.equals(status)) {
            throw new IllegalArgumentException("status must be 'included' or 'ignored'");
        }
    }
}
