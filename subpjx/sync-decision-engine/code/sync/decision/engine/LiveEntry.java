package sync.decision.engine;

import java.time.Instant;
import java.util.Objects;

public record LiveEntry(EntryKind kind, Instant modTime, long byteSize) {
    public LiveEntry {
        Objects.requireNonNull(kind, "kind");
        Objects.requireNonNull(modTime, "modTime");
    }

    public Instant mod_time() {
        return modTime;
    }

    public long byte_size() {
        return byteSize;
    }
}
