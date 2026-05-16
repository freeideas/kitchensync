package sync.decision.engine;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Objects;

public record EntryDecision(
        AuthoritativeState authoritativeState,
        LinkedHashMap<PeerId, List<FilesystemEffect>> filesystemEffects,
        LinkedHashMap<PeerId, List<SnapshotEffect>> snapshotEffects,
        List<PeerId> recursePeers,
        boolean skipped) {
    public EntryDecision {
        Objects.requireNonNull(authoritativeState, "authoritativeState");
        Objects.requireNonNull(filesystemEffects, "filesystemEffects");
        Objects.requireNonNull(snapshotEffects, "snapshotEffects");
        Objects.requireNonNull(recursePeers, "recursePeers");
        filesystemEffects = copyMap(filesystemEffects);
        snapshotEffects = copyMap(snapshotEffects);
        recursePeers = List.copyOf(recursePeers);
    }

    private static <T> LinkedHashMap<PeerId, List<T>> copyMap(LinkedHashMap<PeerId, List<T>> source) {
        LinkedHashMap<PeerId, List<T>> copy = new LinkedHashMap<>();
        for (var entry : source.entrySet()) {
            copy.put(entry.getKey(), List.copyOf(entry.getValue()));
        }
        return copy;
    }

    public AuthoritativeState authoritative_state() {
        return authoritativeState;
    }

    public LinkedHashMap<PeerId, List<FilesystemEffect>> filesystem_effects() {
        return filesystemEffects;
    }

    public LinkedHashMap<PeerId, List<SnapshotEffect>> snapshot_effects() {
        return snapshotEffects;
    }

    public List<PeerId> recurse_peers() {
        return recursePeers;
    }
}
