package kitchensync;

import java.io.OutputStream;
import java.io.PrintStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

import sftp.protocol.SftpPoolRegistry;

public final class Main {
    private Main() {
    }

    public static void main(String[] args) {
        System.setOut(new PrintStream(System.out, true, StandardCharsets.UTF_8));
        System.setErr(new PrintStream(OutputStream.nullOutputStream(), true, StandardCharsets.UTF_8));
        int exit = run(args);
        System.exit(exit);
    }

    static int run(String[] args) {
        CliParser.Parsed parsed;
        try {
            parsed = CliParser.parse(args);
        } catch (CliParser.ValidationException ex) {
            System.out.println("Error: " + ex.getMessage());
            System.out.print(Help.TEXT);
            return 1;
        }
        if (parsed.isHelp()) {
            System.out.print(Help.TEXT);
            return 0;
        }
        RunOptions options = parsed.options();
        try (Logger logger = new Logger(options.verbosity);
                ExecutorService executor = Executors.newCachedThreadPool();
                SftpPoolRegistry pools = new SftpPoolRegistry()) {
            logger.startDirectoryStatus(options.dirStatusSeconds);
            SftpPoolTrace poolTrace = new SftpPoolTrace(logger);
            TimeUtil times = new TimeUtil();
            PeerConnector connector = new PeerConnector(options, pools, logger, poolTrace);
            List<ConnectedPeer> connected = connector.connectAll(executor);
            if (connected.size() < 2) {
                return 1;
            }
            boolean canonRequested = options.peers.stream().anyMatch(p -> p.modifier() == PeerModifier.CANON);
            boolean canonConnected = connected.stream().anyMatch(p -> p.argument().modifier() == PeerModifier.CANON);
            if (canonRequested && !canonConnected) {
                return 1;
            }
            SnapshotManager snapshots = new SnapshotManager(logger, times);
            List<Peer> peers;
            try {
                peers = snapshots.openSnapshots(connected, options, executor);
            } catch (IOException ex) {
                logger.error("snapshot workspace failed");
                return 1;
            }
            if (peers.size() < 2) {
                return 1;
            }
            if (canonRequested && peers.stream().noneMatch(Peer::canon)) {
                return 1;
            }
            boolean anyHistory = peers.stream().anyMatch(p -> p.snapshotHasRows);
            if (!anyHistory && peers.stream().noneMatch(Peer::canon)) {
                System.out.println("First sync? Mark the authoritative peer with a leading +");
                return 1;
            }
            if (peers.stream().noneMatch(p -> !p.subordinate())) {
                System.out.println("No contributing peer reachable - cannot make sync decisions");
                return 1;
            }
            TransferManager transfers = new TransferManager(executor, logger, times);
            new TreeWalker(executor, logger, times, transfers, options).walk(peers);
            transfers.waitForAll();
            for (Peer peer : peers) {
                peer.snapshot.close();
            }
            snapshots.uploadSnapshots(peers);
            for (Peer peer : peers) {
                peer.transport.close();
            }
            return 0;
        }
    }
}
