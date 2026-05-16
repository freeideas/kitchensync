package staged.file.transfer;

import java.time.Instant;
import java.util.Objects;

public record Entry(String name, EntryKind kind, Instant mod_time, long byte_size) {
    public Entry {
        Objects.requireNonNull(name, "name");
        Objects.requireNonNull(kind, "kind");
        Objects.requireNonNull(mod_time, "mod_time");
    }
}
