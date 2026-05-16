package sync.decision.engine;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;

public record DecisionInput(
        String relativePath,
        LinkedHashMap<PeerId, PeerRole> peers,
        Map<PeerId, LiveEntry> liveEntries,
        Map<PeerId, SnapshotRow> snapshotRows) {
    public DecisionInput {
        Objects.requireNonNull(relativePath, "relativePath");
        Objects.requireNonNull(peers, "peers");
        Objects.requireNonNull(liveEntries, "liveEntries");
        Objects.requireNonNull(snapshotRows, "snapshotRows");
        peers = new LinkedHashMap<>(peers);
        liveEntries = Map.copyOf(liveEntries);
        snapshotRows = Map.copyOf(snapshotRows);
    }

    public String relative_path() {
        return relativePath;
    }

    public Map<PeerId, LiveEntry> live_entries() {
        return liveEntries;
    }

    public Map<PeerId, SnapshotRow> snapshot_rows() {
        return snapshotRows;
    }
}
