package kitchensync;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.time.temporal.ChronoUnit;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;

import snapshot.database.SnapshotDatabase;

final class SnapshotManager {
    private static final String SNAPSHOT_PATH = ".kitchensync/snapshot.db";
    private final Logger logger;
    private final TimeUtil times;

    SnapshotManager(Logger logger, TimeUtil times) {
        this.logger = logger;
        this.times = times;
    }

    List<Peer> openSnapshots(List<ConnectedPeer> connected, RunOptions options) throws IOException {
        Path tempRoot = Files.createTempDirectory("kitchensync-");
        List<Peer> peers = new ArrayList<>();
        for (ConnectedPeer connectedPeer : connected) {
            try {
                PeerArgument argument = connectedPeer.argument();
                Path snapshotPath = tempRoot.resolve("peer-" + argument.index()).resolve("snapshot.db");
                Files.createDirectories(snapshotPath.getParent());
                boolean existed = downloadSnapshot(connectedPeer.transport(), snapshotPath);
                SnapshotDatabase db = SnapshotDatabase.open(snapshotPath);
                db.purge(TimeUtil.snapshotTime(Instant.now().minus(options.tombstoneRetentionDays, ChronoUnit.DAYS)));
                boolean hasRows = db.has_rows();
                Peer peer = new Peer(argument.index(), argument.modifier(), connectedPeer.url(), connectedPeer.transport(),
                        snapshotPath, db, existed, hasRows);
                if (!existed && peer.role != sync.decision.engine.PeerRole.CANON) {
                    peer.role = sync.decision.engine.PeerRole.SUBORDINATE;
                }
                peers.add(peer);
            } catch (IOException ex) {
                logger.error("snapshot download failed for " + connectedPeer.url().normalized());
                connectedPeer.transport().close();
            }
        }
        return peers;
    }

    void uploadSnapshots(List<Peer> peers) {
        for (Peer peer : peers) {
            try {
                deleteSnapshotSidecars(peer.transport);
                peer.transport.createDir(".kitchensync");
                String stage = ".kitchensync/TMP/" + times.nextText() + "/" + UUID.randomUUID() + "/snapshot.db";
                writeLocalFileToTransport(peer.localSnapshotPath, peer.transport, stage);
                peer.transport.rename(stage, SNAPSHOT_PATH);
                deleteSnapshotSidecars(peer.transport);
            } catch (TransportException | IOException ex) {
                logger.error("snapshot upload failed for " + peer.url.normalized());
            }
        }
    }

    private static void deleteSnapshotSidecars(Transport transport) throws TransportException {
        deleteIfExists(transport, ".kitchensync/snapshot.db-wal");
        deleteIfExists(transport, ".kitchensync/snapshot.db-shm");
    }

    private static void deleteIfExists(Transport transport, String path) throws TransportException {
        try {
            transport.deleteFile(path);
        } catch (TransportException ex) {
            if (!ex.notFound()) {
                throw ex;
            }
        }
    }

    private boolean downloadSnapshot(Transport transport, Path target) throws IOException {
        try {
            transport.stat(SNAPSHOT_PATH);
        } catch (TransportException ex) {
            if (ex.notFound()) {
                SnapshotDatabase.open(target).close();
                return false;
            }
            throw new IOException(ex);
        }
        try (ReadToken read = transport.openRead(SNAPSHOT_PATH);
                var out = Files.newOutputStream(target)) {
            while (true) {
                byte[] chunk = transport.read(read, 64 * 1024);
                if (chunk.length == 0) {
                    break;
                }
                out.write(chunk);
            }
            return true;
        } catch (TransportException ex) {
            throw new IOException(ex);
        }
    }

    private static void writeLocalFileToTransport(Path source, Transport transport, String target)
            throws IOException, TransportException {
        try (var input = Files.newInputStream(source); WriteToken write = transport.openWrite(target)) {
            byte[] buffer = new byte[64 * 1024];
            while (true) {
                int count = input.read(buffer);
                if (count < 0) {
                    break;
                }
                byte[] chunk = new byte[count];
                System.arraycopy(buffer, 0, chunk, 0, count);
                transport.write(write, chunk);
            }
        }
    }
}
