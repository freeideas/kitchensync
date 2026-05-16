package gitignore.pattern.set;

public final class GitignorePatternSetException extends IllegalArgumentException {
    private final ErrorCategory category;

    public GitignorePatternSetException(ErrorCategory category, String message) {
        super(message);
        this.category = category;
    }

    public ErrorCategory category() {
        return category;
    }
}
