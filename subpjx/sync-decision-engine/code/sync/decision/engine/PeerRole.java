package sync.decision.engine;

public enum PeerRole {
    CANON("canon"),
    NORMAL("normal"),
    SUBORDINATE("subordinate");

    private final String wireName;

    PeerRole(String wireName) {
        this.wireName = wireName;
    }

    public String wireName() {
        return wireName;
    }

    public static PeerRole fromWireName(String value) {
        for (PeerRole role : values()) {
            if (role.wireName.equals(value)) {
                return role;
            }
        }
        throw new IllegalArgumentException("unknown peer role: " + value);
    }
}
