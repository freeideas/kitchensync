package sync.decision.engine;

import java.time.Instant;
import java.util.Objects;

public record SnapshotRow(
        EntryKind kind,
        Instant modTime,
        long byteSize,
        Instant lastSeen,
        Instant deletedTime) {
    public SnapshotRow {
        Objects.requireNonNull(kind, "kind");
        Objects.requireNonNull(modTime, "modTime");
    }

    public Instant mod_time() {
        return modTime;
    }

    public long byte_size() {
        return byteSize;
    }

    public Instant last_seen() {
        return lastSeen;
    }

    public Instant deleted_time() {
        return deletedTime;
    }
}
