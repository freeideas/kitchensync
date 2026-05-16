package sync.decision.engine;

public enum AuthoritativeKind {
    ABSENT("absent"),
    FILE("file"),
    DIRECTORY("directory");

    private final String wireName;

    AuthoritativeKind(String wireName) {
        this.wireName = wireName;
    }

    public String wireName() {
        return wireName;
    }
}
