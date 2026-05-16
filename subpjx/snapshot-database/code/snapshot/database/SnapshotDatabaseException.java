package snapshot.database;

public final class SnapshotDatabaseException extends RuntimeException {
    private final String category;

    public SnapshotDatabaseException(String category, String message) {
        super(message);
        this.category = category;
    }

    public SnapshotDatabaseException(String category, String message, Throwable cause) {
        super(message, cause);
        this.category = category;
    }

    public String category() {
        return category;
    }
}
