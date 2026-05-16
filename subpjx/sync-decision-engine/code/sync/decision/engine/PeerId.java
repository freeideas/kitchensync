package sync.decision.engine;

import java.util.Objects;

public record PeerId(String value) {
    public PeerId {
        Objects.requireNonNull(value, "value");
    }

    @Override
    public String toString() {
        return value;
    }
}
