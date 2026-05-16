package sync.decision.engine;

public enum SnapshotEffect {
    CONFIRM_PRESENT("confirm_present"),
    COPY_PENDING("copy_pending"),
    CREATE_DIRECTORY_CONFIRMED("create_directory_confirmed"),
    MARK_ABSENT("mark_absent"),
    MARK_DISPLACED("mark_displaced"),
    NO_SNAPSHOT_CHANGE("no_snapshot_change");

    private final String wireName;

    SnapshotEffect(String wireName) {
        this.wireName = wireName;
    }

    public String wireName() {
        return wireName;
    }
}
