package snapshot.db;

public record SnapshotRecord(
        String id,
        String parentId,
        String basename,
        String modTime,
        long byteSize,
        String lastSeen,
        String deletedTime
) {}
