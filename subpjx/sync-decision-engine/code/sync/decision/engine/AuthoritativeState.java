package sync.decision.engine;

import java.time.Instant;

public record AuthoritativeState(
        AuthoritativeKind kind,
        PeerId sourcePeer,
        Instant modTime,
        Long byteSize) {
    public static AuthoritativeState absent() {
        return new AuthoritativeState(AuthoritativeKind.ABSENT, null, null, null);
    }

    public static AuthoritativeState file(PeerId sourcePeer, Instant modTime, long byteSize) {
        return new AuthoritativeState(AuthoritativeKind.FILE, sourcePeer, modTime, byteSize);
    }

    public static AuthoritativeState directory(PeerId sourcePeer) {
        return new AuthoritativeState(AuthoritativeKind.DIRECTORY, sourcePeer, null, null);
    }

    public PeerId source_peer() {
        return sourcePeer;
    }

    public Instant mod_time() {
        return modTime;
    }

    public Long byte_size() {
        return byteSize;
    }
}
