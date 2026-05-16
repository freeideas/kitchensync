package snapshot.database;

public record EntryMetadata(EntryKind kind, SnapshotTime mod_time, long byte_size) {
    public EntryMetadata {
        if (kind == null || mod_time == null) {
            throw new SnapshotDatabaseException("invalid_metadata", "metadata is required");
        }
        if (kind == EntryKind.FILE && byte_size < 0) {
            throw new SnapshotDatabaseException("invalid_metadata", "file size is negative");
        }
        if (kind == EntryKind.DIRECTORY && byte_size != -1) {
            throw new SnapshotDatabaseException("invalid_metadata", "directory size must be -1");
        }
    }
}
