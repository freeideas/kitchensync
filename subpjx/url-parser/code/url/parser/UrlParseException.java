package url.parser;

public final class UrlParseException extends RuntimeException {
    private final ParseErrorCategory category;

    public UrlParseException(ParseErrorCategory category, String detail) {
        super(detail == null || detail.isEmpty() ? category.name() : category.name() + ": " + detail);
        this.category = category;
    }

    public ParseErrorCategory category() {
        return category;
    }
}
