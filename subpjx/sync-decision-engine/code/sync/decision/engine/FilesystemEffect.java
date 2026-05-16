package sync.decision.engine;

public enum FilesystemEffect {
    KEEP("keep"),
    COPY_FILE("copy_file"),
    CREATE_DIRECTORY("create_directory"),
    DISPLACE("displace");

    private final String wireName;

    FilesystemEffect(String wireName) {
        this.wireName = wireName;
    }

    public String wireName() {
        return wireName;
    }
}
