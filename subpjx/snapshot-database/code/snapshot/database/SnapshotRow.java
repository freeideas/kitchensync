package snapshot.database;

import java.util.Optional;

public record SnapshotRow(
        String id,
        String parent_id,
        String relative_path,
        String basename,
        EntryKind kind,
        SnapshotTime mod_time,
        long byte_size,
        Optional<SnapshotTime> last_seen,
        Optional<SnapshotTime> deleted_time) {
    public SnapshotRow {
        if (id == null || parent_id == null || relative_path == null || basename == null
                || kind == null || mod_time == null || last_seen == null || deleted_time == null) {
            throw new SnapshotDatabaseException("database_error", "row field is required");
        }
    }
}
