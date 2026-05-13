package decision.engine;

public record HistoryRecord(long modTime, long byteSize, Long lastSeen, Long deletedTime) {
    public boolean tombstone() {
        return deletedTime != null;
    }

    public long getModTime() {
        return modTime;
    }

    public long getByteSize() {
        return byteSize;
    }

    public Long getLastSeen() {
        return lastSeen;
    }

    public Long getDeletedTime() {
        return deletedTime;
    }
}
