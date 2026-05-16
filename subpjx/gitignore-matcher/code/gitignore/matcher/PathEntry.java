package gitignore.matcher;

import java.util.Objects;

public record PathEntry(String relativePath, EntryKind kind) {
    public PathEntry {
        Objects.requireNonNull(relativePath, "relativePath");
        Objects.requireNonNull(kind, "kind");
    }
}
