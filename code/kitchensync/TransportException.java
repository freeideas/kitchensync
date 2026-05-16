package kitchensync;

final class TransportException extends Exception {
    enum Category {
        NOT_FOUND,
        PERMISSION_DENIED,
        IO_ERROR
    }

    private final Category category;

    TransportException(Category category, String message) {
        super(message);
        this.category = category;
    }

    TransportException(Category category, String message, Throwable cause) {
        super(message, cause);
        this.category = category;
    }

    Category category() {
        return category;
    }

    boolean notFound() {
        return category == Category.NOT_FOUND;
    }
}
