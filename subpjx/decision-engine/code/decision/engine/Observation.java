package decision.engine;

import java.util.Objects;

public final class Observation {
    public enum Kind {
        FILE,
        DIRECTORY,
        ABSENT
    }

    private final Kind kind;
    private final Long modTime;
    private final Long byteSize;

    private Observation(Kind kind, Long modTime, Long byteSize) {
        this.kind = Objects.requireNonNull(kind, "kind");
        this.modTime = modTime;
        this.byteSize = byteSize;
    }

    public static Observation file(long modTime, long byteSize) {
        return new Observation(Kind.FILE, modTime, byteSize);
    }

    public static Observation directory() {
        return new Observation(Kind.DIRECTORY, null, null);
    }

    public static Observation absent() {
        return new Observation(Kind.ABSENT, null, null);
    }

    public Kind kind() {
        return kind;
    }

    public Kind getKind() {
        return kind;
    }

    public Long modTime() {
        return modTime;
    }

    public Long getModTime() {
        return modTime;
    }

    public Long byteSize() {
        return byteSize;
    }

    public Long getByteSize() {
        return byteSize;
    }

    public long requireModTime() {
        if (modTime == null) {
            throw new IllegalStateException("observation has no mod_time");
        }
        return modTime;
    }

    public long requireByteSize() {
        if (byteSize == null) {
            throw new IllegalStateException("observation has no byte_size");
        }
        return byteSize;
    }

    @Override
    public boolean equals(Object other) {
        if (this == other) {
            return true;
        }
        if (!(other instanceof Observation observation)) {
            return false;
        }
        return kind == observation.kind
                && Objects.equals(modTime, observation.modTime)
                && Objects.equals(byteSize, observation.byteSize);
    }

    @Override
    public int hashCode() {
        return Objects.hash(kind, modTime, byteSize);
    }

    @Override
    public String toString() {
        return "Observation[kind=" + kind + ", modTime=" + modTime + ", byteSize=" + byteSize + "]";
    }
}
