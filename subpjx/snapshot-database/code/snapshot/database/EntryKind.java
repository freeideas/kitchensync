package snapshot.database;

public enum EntryKind {
    FILE("file"),
    DIRECTORY("directory");

    private final String wireName;

    EntryKind(String wireName) {
        this.wireName = wireName;
    }

    public String wireName() {
        return wireName;
    }

    public static EntryKind fromWireName(String value) {
        for (EntryKind kind : values()) {
            if (kind.wireName.equals(value)) {
                return kind;
            }
        }
        throw new SnapshotDatabaseException("invalid_metadata", "invalid entry kind");
    }
}
