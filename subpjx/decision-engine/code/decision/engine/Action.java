package decision.engine;

import java.util.Objects;

public final class Action {
    public enum Kind {
        NO_OP,
        RECEIVE_FILE,
        CREATE_DIRECTORY,
        DISPLACE
    }

    private static final Action NO_OP = new Action(Kind.NO_OP, null);
    private static final Action CREATE_DIRECTORY = new Action(Kind.CREATE_DIRECTORY, null);
    private static final Action DISPLACE = new Action(Kind.DISPLACE, null);

    private final Kind kind;
    private final String source;

    private Action(Kind kind, String source) {
        this.kind = Objects.requireNonNull(kind, "kind");
        this.source = source;
    }

    public static Action noOp() {
        return NO_OP;
    }

    public static Action receiveFile(String source) {
        return new Action(Kind.RECEIVE_FILE, Objects.requireNonNull(source, "source"));
    }

    public static Action createDirectory() {
        return CREATE_DIRECTORY;
    }

    public static Action displace() {
        return DISPLACE;
    }

    public Kind kind() {
        return kind;
    }

    public Kind getKind() {
        return kind;
    }

    public String source() {
        return source;
    }

    public String getSource() {
        return source;
    }

    @Override
    public boolean equals(Object other) {
        if (this == other) {
            return true;
        }
        if (!(other instanceof Action action)) {
            return false;
        }
        return kind == action.kind && Objects.equals(source, action.source);
    }

    @Override
    public int hashCode() {
        return Objects.hash(kind, source);
    }

    @Override
    public String toString() {
        return source == null ? kind.toString() : kind + "(" + source + ")";
    }
}
