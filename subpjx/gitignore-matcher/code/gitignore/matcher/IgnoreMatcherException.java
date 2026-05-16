package gitignore.matcher;

public final class IgnoreMatcherException extends IllegalArgumentException {
    private final String category;

    public IgnoreMatcherException(String category, String message) {
        super(category + ": " + message);
        this.category = category;
    }

    public String category() {
        return category;
    }
}
