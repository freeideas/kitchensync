package kitchensync;

import java.nio.file.Path;
import java.util.concurrent.Semaphore;

import snapshot.database.SnapshotDatabase;
import sync.decision.engine.PeerId;
import sync.decision.engine.PeerRole;

final class Peer {
    final PeerId id;
    final int index;
    final PeerModifier declaredModifier;
    PeerRole role;
    final PeerUrl url;
    final Transport transport;
    final Path localSnapshotPath;
    final SnapshotDatabase snapshot;
    final boolean existingSnapshotFile;
    final boolean snapshotHasRows;
    final Semaphore transferPermits;

    Peer(int index, PeerModifier declaredModifier, PeerUrl url, Transport transport, Path localSnapshotPath,
            SnapshotDatabase snapshot, boolean existingSnapshotFile, boolean snapshotHasRows) {
        this.id = new PeerId("p" + index);
        this.index = index;
        this.declaredModifier = declaredModifier;
        this.role = switch (declaredModifier) {
            case CANON -> PeerRole.CANON;
            case SUBORDINATE -> PeerRole.SUBORDINATE;
            case NORMAL -> PeerRole.NORMAL;
        };
        this.url = url;
        this.transport = transport;
        this.localSnapshotPath = localSnapshotPath;
        this.snapshot = snapshot;
        this.existingSnapshotFile = existingSnapshotFile;
        this.snapshotHasRows = snapshotHasRows;
        this.transferPermits = new Semaphore(Math.max(1, url.config().maxConnections()));
    }

    boolean subordinate() {
        return role == PeerRole.SUBORDINATE;
    }

    boolean canon() {
        return role == PeerRole.CANON;
    }
}
