package kitchensync;

import decision.engine.Action;
import decision.engine.Decision;
import decision.engine.DecisionEngine;
import decision.engine.HistoryRecord;
import decision.engine.Observation;
import gitignore.matcher.GitignoreMatcher;
import gitignore.matcher.MatchResult;
import gitignore.matcher.StackEntry;
import sftp.protocol.ConnectionHandle;
import sftp.protocol.DirEntry;
import sftp.protocol.SftpNotFoundException;
import sftp.protocol.SftpPool;
import sftp.protocol.SftpPoolConfig;
import sftp.protocol.StatResult;
import snapshot.db.SnapshotRecord;
import snapshot.db.SnapshotStore;
import snapshot.db.Timestamps;
import url.parser.ParsedUrl;
import url.parser.Role;
import url.parser.TaggedGroup;
import url.parser.UrlParser;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.attribute.BasicFileAttributes;
import java.nio.file.attribute.FileTime;
import java.sql.SQLException;
import java.time.Instant;
import java.time.ZoneOffset;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.UUID;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

public final class Main {
    private static final String FIRST_SYNC_MESSAGE = "First sync? Mark the authoritative peer with a leading +";
    private static final String NO_CONTRIBUTORS_MESSAGE = "No contributing peer reachable — cannot make sync decisions";
    private static final int CHUNK_SIZE = 64 * 1024;
    private static final byte[] EOF = new byte[0];
    private static final DateTimeFormatter SNAPSHOT_TIME =
            DateTimeFormatter.ofPattern("yyyy-MM-dd_HH-mm-ss_SSSSSS'Z'", Locale.ROOT).withZone(ZoneOffset.UTC);

    private Main() {
    }

    public static void main(String[] args) {
        System.setErr(new PrintStream(OutputStream.nullOutputStream(), true, StandardCharsets.UTF_8));
        int exit = new Main().run(args);
        System.exit(exit);
    }

    int run(String[] args) {
        Cli cli = Cli.parse(args);
        if (cli.help) {
            System.out.print(Help.TEXT);
            return 0;
        }
        if (cli.error != null) {
            System.out.println(cli.error);
            System.out.print(Help.TEXT);
            return 1;
        }

        Logger log = new Logger(cli.options.verbosity);
        ExecutorService io = Executors.newCachedThreadPool();
        SftpPools sftpPools = new SftpPools(log);
        Path tempRoot = null;
        List<Peer> connected = new ArrayList<>();
        try {
            tempRoot = Files.createTempDirectory("kitchensync-");
            List<PeerSpec> specs = cli.peers;
            List<Future<Peer>> futures = new ArrayList<>();
            for (int i = 0; i < specs.size(); i++) {
                final int index = i;
                futures.add(io.submit(() -> connectPeer("p" + index, specs.get(index), cli.options, sftpPools)));
            }
            for (Future<Peer> future : futures) {
                try {
                    Peer peer = future.get();
                    if (peer != null) {
                        connected.add(peer);
                    }
                } catch (Exception ex) {
                    log.error("peer unreachable: " + rootMessage(ex));
                }
            }
            if (connected.size() < 2) {
                closePeers(connected);
                return 1;
            }
            if (specs.stream().anyMatch(PeerSpec::canon) && connected.stream().noneMatch(p -> p.explicitRole == url.parser.Role.CANON)) {
                closePeers(connected);
                return 1;
            }

            List<Peer> reachable = downloadSnapshots(connected, tempRoot, log);
            if (reachable.size() < 2) {
                closePeers(reachable);
                return 1;
            }
            if (specs.stream().anyMatch(PeerSpec::canon) && reachable.stream().noneMatch(p -> p.explicitRole == url.parser.Role.CANON)) {
                closePeers(reachable);
                return 1;
            }

            boolean anyExistingSnapshot = reachable.stream().anyMatch(p -> p.snapshotExisted);
            for (Peer peer : reachable) {
                peer.subordinate = peer.explicitRole == url.parser.Role.SUBORDINATE
                        || (!peer.snapshotExisted && peer.explicitRole != url.parser.Role.CANON);
            }
            if (!anyExistingSnapshot && reachable.stream().noneMatch(p -> p.explicitRole == url.parser.Role.CANON)) {
                System.out.println(FIRST_SYNC_MESSAGE);
                closePeers(reachable);
                return 1;
            }
            if (reachable.stream().noneMatch(Peer::contributes)) {
                System.out.println(NO_CONTRIBUTORS_MESSAGE);
                closePeers(reachable);
                return 1;
            }

            for (Peer peer : reachable) {
                peer.store.purgeOlderThan(cli.options.tombstoneDays, Timestamps.now());
            }

            SyncEngine engine = new SyncEngine(reachable, cli.options, log, io);
            engine.syncDirectory("", List.of(new StackEntry("", GitignoreMatcher.compile(".git/\n"))));
            engine.awaitTransfers();
            uploadSnapshots(reachable, log);
            closePeers(reachable);
            return 0;
        } catch (Exception ex) {
            log.error(rootMessage(ex));
            closePeers(connected);
            return 1;
        } finally {
            sftpPools.shutdown();
            io.shutdownNow();
            if (tempRoot != null) {
                deleteRecursively(tempRoot);
            }
        }
    }

    private static Peer connectPeer(String id, PeerSpec spec, Options options, SftpPools sftpPools) {
        RuntimeException last = null;
        for (ParsedUrl url : spec.urls) {
            try {
                Transport transport;
                if ("file".equals(url.scheme())) {
                    transport = new LocalTransport(Path.of(url.path()));
                } else if ("sftp".equals(url.scheme())) {
                    transport = new SftpTransport(url, sftpPools, options);
                } else {
                    throw new SyncException("unsupported scheme: " + url.scheme());
                }
                TransportConnection listingConnection = transport.openListingConnection();
                try {
                    listingConnection.createDir("");
                    return new Peer(id, spec.role, url, transport, listingConnection);
                } catch (Exception ex) {
                    try {
                        listingConnection.close();
                    } catch (Exception ignored) {
                    }
                    throw ex;
                }
            } catch (RuntimeException ex) {
                last = ex;
            } catch (Exception ex) {
                last = new SyncException(rootMessage(ex), ex);
            }
        }
        throw last == null ? new SyncException("unreachable peer") : last;
    }

    private static List<Peer> downloadSnapshots(List<Peer> peers, Path tempRoot, Logger log) throws IOException {
        List<Peer> reachable = new ArrayList<>();
        for (Peer peer : peers) {
            Path snapshotDir = tempRoot.resolve(UUID.randomUUID().toString());
            Files.createDirectories(snapshotDir);
            Path db = snapshotDir.resolve("snapshot.db");
            try {
                byte[] bytes;
                try (TransportConnection conn = peer.transport.borrow()) {
                    bytes = conn.readAll(".kitchensync/snapshot.db");
                }
                Files.write(db, bytes);
                peer.snapshotExisted = true;
            } catch (NotFoundException ex) {
                peer.snapshotExisted = false;
            } catch (Exception ex) {
                log.error("peer unreachable: snapshot download failed for " + peer.identity() + ": " + rootMessage(ex));
                closeQuietly(peer);
                continue;
            }
            try {
                peer.snapshotPath = db;
                peer.store = SnapshotStore.open(db.toString());
                reachable.add(peer);
            } catch (SQLException ex) {
                log.error("peer unreachable: snapshot open failed for " + peer.identity() + ": " + rootMessage(ex));
                closeQuietly(peer);
            }
        }
        return reachable;
    }

    private static void uploadSnapshots(List<Peer> peers, Logger log) {
        for (Peer peer : peers) {
            try {
                if (peer.store != null) {
                    peer.store.close();
                    peer.store = null;
                }
                byte[] bytes = Files.readAllBytes(peer.snapshotPath);
                try (TransportConnection conn = peer.transport.borrow()) {
                    String tmp = ".kitchensync/TMP/" + Timestamps.now() + "/" + UUID.randomUUID() + "/snapshot.db";
                    conn.writeAll(tmp, bytes);
                    conn.createDir(".kitchensync");
                    conn.rename(tmp, ".kitchensync/snapshot.db");
                    cleanupEmptyTransferDirs(conn, tmp);
                }
            } catch (Exception ex) {
                log.error("snapshot upload failed for " + peer.identity() + ": " + rootMessage(ex));
            }
        }
    }

    private static void closePeers(List<Peer> peers) {
        for (Peer peer : peers) {
            closeQuietly(peer);
        }
    }

    private static void closeQuietly(Peer peer) {
        try {
            if (peer.store != null) {
                peer.store.close();
                peer.store = null;
            }
        } catch (Exception ignored) {
        }
        try {
            peer.listingConnection.close();
        } catch (Exception ignored) {
        }
        try {
            peer.transport.close();
        } catch (Exception ignored) {
        }
    }

    private static String rootMessage(Throwable throwable) {
        Throwable current = throwable;
        while (current instanceof CompletionException && current.getCause() != null) {
            current = current.getCause();
        }
        return current.getMessage() == null ? current.getClass().getSimpleName() : current.getMessage();
    }

    private static String child(String parent, String name) {
        return parent == null || parent.isEmpty() ? name : parent + "/" + name;
    }

    private static String parentOf(String rel) {
        int slash = rel.lastIndexOf('/');
        return slash < 0 ? "" : rel.substring(0, slash);
    }

    private static String basename(String rel) {
        int slash = rel.lastIndexOf('/');
        return slash < 0 ? rel : rel.substring(slash + 1);
    }

    private static String formatInstant(Instant instant) {
        return SNAPSHOT_TIME.format(instant);
    }

    private static String formatEpochSeconds(long seconds) {
        return formatInstant(Instant.ofEpochSecond(seconds));
    }

    private static long parseSnapshotSeconds(String timestamp) {
        if (timestamp == null || timestamp.isEmpty()) {
            return 0L;
        }
        return Instant.from(SNAPSHOT_TIME.parse(timestamp)).getEpochSecond();
    }

    private static void deleteRecursively(Path path) {
        if (!Files.exists(path, LinkOption.NOFOLLOW_LINKS)) {
            return;
        }
        try {
            Files.walk(path)
                    .sorted(Comparator.reverseOrder())
                    .forEach(p -> {
                        try {
                            Files.deleteIfExists(p);
                        } catch (IOException ignored) {
                        }
                    });
        } catch (IOException ignored) {
        }
    }

    private static void cleanupEmptyTransferDirs(TransportConnection conn, String tmpFile) {
        String uuidDir = parentOf(tmpFile);
        String timestampDir = parentOf(uuidDir);
        try {
            conn.deleteDir(uuidDir);
        } catch (Exception ignored) {
        }
        try {
            conn.deleteDir(timestampDir);
        } catch (Exception ignored) {
        }
    }

    private static final class SyncEngine {
        private final List<Peer> peers;
        private final Options options;
        private final Logger log;
        private final ExecutorService io;
        private final List<Future<?>> transfers = new ArrayList<>();
        private final Set<String> loggedCopies = new HashSet<>();
        private final Set<String> loggedDisplacements = new HashSet<>();

        private SyncEngine(List<Peer> peers, Options options, Logger log, ExecutorService io) {
            this.peers = peers;
            this.options = options;
            this.log = log;
            this.io = io;
        }

        void syncDirectory(String dir, List<StackEntry> ignoreStack) {
            syncDirectory(peers, dir, ignoreStack);
        }

        private void syncDirectory(List<Peer> scope, String dir, List<StackEntry> ignoreStack) {
            Map<Peer, ListingResult> listings = listPeersConcurrently(scope, dir);
            List<Peer> active = new ArrayList<>();
            for (Peer peer : scope) {
                ListingResult result = listings.get(peer);
                if (result.error == null) {
                    active.add(peer);
                } else {
                    log.error("listing failed for " + peer.identity() + " at " + printable(dir) + ": " + rootMessage(result.error));
                }
            }
            if (active.stream().noneMatch(Peer::contributes)) {
                return;
            }

            TreeSet<String> names = new TreeSet<>();
            for (Peer peer : active) {
                names.addAll(listings.get(peer).entries.keySet());
            }

            List<StackEntry> effectiveIgnore = ignoreStack;
            if (names.contains(".syncignore")) {
                DecisionContext syncignore = decide(active, listings, dir, ".syncignore");
                apply(syncignore, active, listings, dir, ".syncignore");
                names.remove(".syncignore");
                Optional<String> text = readWinningSyncignore(syncignore, ".syncignore");
                if (text.isPresent()) {
                    effectiveIgnore = new ArrayList<>(ignoreStack);
                    effectiveIgnore.add(new StackEntry(dir, GitignoreMatcher.compile(text.get())));
                } else if (syncignore.decision.winningSource() != null) {
                    log.error("failed to read .syncignore at " + printable(dir));
                }
            }

            List<String> directoriesToRecurse = new ArrayList<>();
            for (String name : names) {
                String rel = child(dir, name);
                FsEntry visible = firstEntry(active, listings, name);
                if (visible != null && ignored(effectiveIgnore, rel, visible.directory)) {
                    continue;
                }
                DecisionContext context = decide(active, listings, dir, name);
                apply(context, active, listings, dir, name);
                if (context.decision.entryKind() == decision.engine.EntryKind.DIRECTORY) {
                    directoriesToRecurse.add(name);
                }
            }

            cleanupMetadata(active, dir);
            for (String name : directoriesToRecurse) {
                String rel = child(dir, name);
                List<Peer> recursionPeers = new ArrayList<>();
                for (Peer peer : active) {
                    Action action = latestDecisionAction(peer, rel);
                    FsEntry entry = listings.get(peer).entries.get(name);
                    if (action != null && action.kind() == Action.Kind.DISPLACE) {
                        continue;
                    }
                    if ((entry != null && entry.directory) || (action != null && action.kind() == Action.Kind.CREATE_DIRECTORY)) {
                        recursionPeers.add(peer);
                    }
                }
                if (!recursionPeers.isEmpty()) {
                    syncDirectory(recursionPeers, rel, effectiveIgnore);
                }
            }
        }

        private Action latestDecisionAction(Peer peer, String rel) {
            return peer.lastActions.get(rel);
        }

        private Map<Peer, ListingResult> listPeersConcurrently(List<Peer> scope, String dir) {
            Map<Peer, CompletableFuture<ListingResult>> futures = new LinkedHashMap<>();
            for (Peer peer : scope) {
                futures.put(peer, CompletableFuture.supplyAsync(() -> {
                    try {
                        return ListingResult.success(peer.listingConnection.listDir(dir));
                    } catch (Exception ex) {
                        return ListingResult.failure(ex);
                    }
                }, io));
            }
            Map<Peer, ListingResult> results = new LinkedHashMap<>();
            for (Map.Entry<Peer, CompletableFuture<ListingResult>> entry : futures.entrySet()) {
                results.put(entry.getKey(), entry.getValue().join());
            }
            return results;
        }

        private DecisionContext decide(List<Peer> active, Map<Peer, ListingResult> listings, String dir, String name) {
            String rel = child(dir, name);
            Map<String, decision.engine.Role> roles = new TreeMap<>();
            Map<String, Observation> observations = new TreeMap<>();
            Map<String, HistoryRecord> histories = new TreeMap<>();
            Map<Peer, Optional<SnapshotRecord>> records = new LinkedHashMap<>();
            for (Peer peer : active) {
                roles.put(peer.id, peer.decisionRole());
                FsEntry entry = listings.get(peer).entries.get(name);
                observations.put(peer.id, observationOf(entry));
                Optional<SnapshotRecord> record = lookup(peer, rel);
                records.put(peer, record);
                record.ifPresent(r -> histories.put(peer.id, historyOf(r)));
            }
            Decision decision = DecisionEngine.decideEntry(roles, observations, histories);
            return new DecisionContext(rel, decision, records);
        }

        private void apply(DecisionContext context, List<Peer> active, Map<Peer, ListingResult> listings, String dir, String name) {
            String rel = context.rel;
            boolean copyLogged = false;
            boolean displaceLogged = false;
            for (Peer peer : active) {
                Action action = context.decision.actions().get(peer.id);
                FsEntry entry = listings.get(peer).entries.get(name);
                if (action != null) {
                    peer.lastActions.put(rel, action);
                    if (action.kind() == Action.Kind.RECEIVE_FILE) {
                        copyLogged = true;
                        if (entry != null) {
                            displaceLogged = true;
                        }
                    } else if (action.kind() == Action.Kind.DISPLACE) {
                        displaceLogged = true;
                        if (context.decision.entryKind() == decision.engine.EntryKind.FILE) {
                            copyLogged = true;
                        }
                    }
                }
            }
            if (copyLogged && loggedCopies.add(rel)) {
                log.info("C " + rel);
            }
            if (displaceLogged && loggedDisplacements.add(rel)) {
                log.info("X " + rel);
            }

            for (Peer peer : active) {
                FsEntry entry = listings.get(peer).entries.get(name);
                Action action = context.decision.actions().get(peer.id);
                if (action != null && action.kind() != Action.Kind.NO_OP) {
                    continue;
                }
                Optional<SnapshotRecord> record = context.records.getOrDefault(peer, Optional.empty());
                try {
                    if (entry != null) {
                        peer.store.upsertObserved(rel, formatInstant(entry.modTime), entry.byteSize, entry.directory, Timestamps.now());
                    } else if (record.isPresent() && record.get().deletedTime() == null) {
                        peer.store.markSubtreeDeleted(rel, record.get().lastSeen());
                    }
                } catch (SQLException ex) {
                    throw new SyncException(rootMessage(ex), ex);
                }
            }

            for (Peer peer : active) {
                Action action = context.decision.actions().get(peer.id);
                FsEntry entry = listings.get(peer).entries.get(name);
                if (action == null || action.kind() == Action.Kind.NO_OP) {
                    continue;
                }
                try {
                    if (action.kind() == Action.Kind.CREATE_DIRECTORY) {
                        createWinningDirectory(context, peer, rel);
                    } else if (action.kind() == Action.Kind.DISPLACE) {
                        if (displace(peer, rel)) {
                            Optional<SnapshotRecord> record = context.records.getOrDefault(peer, Optional.empty());
                            peer.store.markSubtreeDeleted(rel, deletionTimestamp(record));
                            if (context.decision.entryKind() == decision.engine.EntryKind.FILE) {
                                enqueueWinningFile(context, peer, rel);
                            } else if (context.decision.entryKind() == decision.engine.EntryKind.DIRECTORY) {
                                createWinningDirectory(context, peer, rel);
                                peer.lastActions.put(rel, Action.createDirectory());
                            }
                        }
                    } else if (action.kind() == Action.Kind.RECEIVE_FILE) {
                        if (context.decision.winningModTime() != null && context.decision.winningByteSize() != null) {
                            if (entry != null && entry.directory) {
                                Optional<SnapshotRecord> record = context.records.getOrDefault(peer, Optional.empty());
                                peer.store.markSubtreeDeleted(rel, deletionTimestamp(record));
                            }
                            enqueueWinningFile(context, peer, rel);
                        }
                    }
                } catch (Exception ex) {
                    log.error("operation failed for " + rel + " on " + peer.identity() + ": " + rootMessage(ex));
                }
            }
        }

        private String deletionTimestamp(Optional<SnapshotRecord> record) {
            return record.isPresent() ? record.get().lastSeen() : Timestamps.now();
        }

        private void createWinningDirectory(DecisionContext context, Peer peer, String rel) throws Exception {
            try (TransportConnection conn = peer.transport.borrow()) {
                conn.createDir(rel);
            }
            peer.store.recordDecided(rel, decisionModTime(context), -1, true);
            peer.store.confirmPresent(rel, Timestamps.now());
        }

        private void enqueueWinningFile(DecisionContext context, Peer peer, String rel) throws SQLException {
            Peer source = findPeer(context.decision.winningSource());
            if (source != null && context.decision.winningModTime() != null && context.decision.winningByteSize() != null) {
                peer.store.recordDecided(rel, formatEpochSeconds(context.decision.winningModTime()), context.decision.winningByteSize(), false);
                transfers.add(io.submit(() -> transfer(source, peer, rel, Instant.ofEpochSecond(context.decision.winningModTime()))));
            }
        }

        private Peer findPeer(String id) {
            for (Peer peer : peers) {
                if (peer.id.equals(id)) {
                    return peer;
                }
            }
            return null;
        }

        void awaitTransfers() {
            for (Future<?> transfer : transfers) {
                try {
                    transfer.get();
                } catch (Exception ex) {
                    log.error("transfer failed: " + rootMessage(ex));
                }
            }
        }

        private void transfer(Peer sourcePeer, Peer destPeer, String rel, Instant winningModTime) {
            String parent = parentOf(rel);
            String tmp = child(child(child(parent, ".kitchensync/TMP"), Timestamps.now()), UUID.randomUUID() + "/" + basename(rel));
            try (TransportConnection source = sourcePeer.transport.borrow();
                 TransportConnection dest = destPeer.transport.borrow()) {
                ArrayBlockingQueue<byte[]> channel = new ArrayBlockingQueue<>(8);
                Future<?> reader = io.submit(() -> {
                    try (ReadStream in = source.openRead(rel)) {
                        while (true) {
                            byte[] chunk = in.read(CHUNK_SIZE);
                            if (chunk.length == 0) {
                                channel.put(EOF);
                                return;
                            }
                            channel.put(chunk);
                        }
                    } catch (Exception ex) {
                        try {
                            channel.put(EOF);
                        } catch (InterruptedException interrupted) {
                            Thread.currentThread().interrupt();
                        }
                        throw new CompletionException(ex);
                    }
                });
                Future<?> writer = io.submit(() -> {
                    try (WriteStream out = dest.openWrite(tmp)) {
                        while (true) {
                            byte[] chunk = channel.take();
                            if (chunk == EOF) {
                                return;
                            }
                            out.write(chunk);
                        }
                    } catch (Exception ex) {
                        throw new CompletionException(ex);
                    }
                });
                waitForPump(reader, writer);
                try {
                    dest.stat(rel);
                    if (!displace(destPeer, dest, rel)) {
                        throw new SyncException("displacement failed");
                    }
                } catch (NotFoundException ignored) {
                }
                dest.rename(tmp, rel);
                try {
                    dest.setModTime(rel, winningModTime);
                } catch (Exception ex) {
                    log.error("set_mod_time failed for " + rel + " on " + destPeer.identity() + ": " + rootMessage(ex));
                }
                cleanupEmptyTransferDirs(dest, tmp);
                destPeer.store.confirmPresent(rel, Timestamps.now());
            } catch (Exception ex) {
                try (TransportConnection conn = destPeer.transport.borrow()) {
                    String uuidDir = parentOf(tmp);
                    conn.deleteTree(uuidDir);
                    try {
                        conn.deleteDir(parentOf(uuidDir));
                    } catch (Exception ignored) {
                    }
                } catch (Exception ignored) {
                }
                log.error("transfer failed for " + rel + ": " + rootMessage(ex));
            }
        }

        private void waitForPump(Future<?> reader, Future<?> writer) throws Exception {
            while (true) {
                if (reader.isDone()) {
                    try {
                        reader.get();
                    } catch (Exception ex) {
                        writer.cancel(true);
                        throw ex;
                    }
                }
                if (writer.isDone()) {
                    try {
                        writer.get();
                    } catch (Exception ex) {
                        reader.cancel(true);
                        throw ex;
                    }
                }
                if (reader.isDone() && writer.isDone()) {
                    return;
                }
                Thread.sleep(10L);
            }
        }

        private boolean displace(Peer peer, String rel) {
            try (TransportConnection conn = peer.transport.borrow()) {
                return displace(peer, conn, rel);
            } catch (Exception ex) {
                log.error("displacement failed for " + rel + " on " + peer.identity() + ": " + rootMessage(ex));
                return false;
            }
        }

        private boolean displace(Peer peer, TransportConnection conn, String rel) {
            String dest = child(child(parentOf(rel), ".kitchensync/BAK/" + Timestamps.now()), basename(rel));
            try {
                conn.createDir(parentOf(dest));
                conn.rename(rel, dest);
                return true;
            } catch (Exception ex) {
                log.error("displacement failed for " + rel + " on " + peer.identity() + ": " + rootMessage(ex));
                return false;
            }
        }

        private Optional<String> readWinningSyncignore(DecisionContext context, String name) {
            String sourceId = context.decision.winningSource();
            if (sourceId == null) {
                return Optional.empty();
            }
            Peer source = findPeer(sourceId);
            if (source == null) {
                return Optional.empty();
            }
            try (TransportConnection conn = source.transport.borrow()) {
                return Optional.of(new String(conn.readAll(context.rel), StandardCharsets.UTF_8));
            } catch (Exception ex) {
                return Optional.empty();
            }
        }

        private void cleanupMetadata(List<Peer> active, String dir) {
            for (Peer peer : active) {
                try (TransportConnection conn = peer.transport.borrow()) {
                    purgeOld(conn, child(dir, ".kitchensync/BAK"), options.backupDays);
                    purgeOld(conn, child(dir, ".kitchensync/TMP"), options.tmpDays);
                } catch (Exception ignored) {
                }
            }
        }

        private void purgeOld(TransportConnection conn, String root, int days) {
            Instant cutoff = Instant.now().minusSeconds(days * 86_400L);
            Map<String, FsEntry> entries;
            try {
                entries = conn.listDir(root);
            } catch (Exception ex) {
                return;
            }
            for (String name : entries.keySet()) {
                try {
                    Instant stamp = Instant.from(SNAPSHOT_TIME.parse(name));
                    if (stamp.isBefore(cutoff)) {
                        conn.deleteTree(child(root, name));
                    }
                } catch (Exception ignored) {
                }
            }
        }

        private boolean ignored(List<StackEntry> stack, String rel, boolean dir) {
            return GitignoreMatcher.match(stack, rel, dir) == MatchResult.IGNORED;
        }

        private FsEntry firstEntry(List<Peer> active, Map<Peer, ListingResult> listings, String name) {
            for (Peer peer : active) {
                FsEntry entry = listings.get(peer).entries.get(name);
                if (entry != null) {
                    return entry;
                }
            }
            return null;
        }

        private Observation observationOf(FsEntry entry) {
            if (entry == null) {
                return Observation.absent();
            }
            if (entry.directory) {
                return Observation.directory();
            }
            return Observation.file(entry.modTime.getEpochSecond(), entry.byteSize);
        }

        private Optional<SnapshotRecord> lookup(Peer peer, String rel) {
            try {
                return peer.store.lookup(rel);
            } catch (SQLException ex) {
                throw new SyncException(rootMessage(ex), ex);
            }
        }

        private HistoryRecord historyOf(SnapshotRecord record) {
            return new HistoryRecord(
                    parseSnapshotSeconds(record.modTime()),
                    record.byteSize(),
                    record.lastSeen() == null ? null : parseSnapshotSeconds(record.lastSeen()),
                    record.deletedTime() == null ? null : parseSnapshotSeconds(record.deletedTime()));
        }

        private String decisionModTime(DecisionContext context) {
            Long seconds = context.decision.winningModTime();
            return seconds == null ? Timestamps.now() : formatEpochSeconds(seconds);
        }

        private String printable(String dir) {
            return dir == null || dir.isEmpty() ? "/" : dir;
        }
    }

    private interface Transport extends AutoCloseable {
        TransportConnection borrow() throws Exception;

        default TransportConnection openListingConnection() throws Exception {
            return borrow();
        }

        @Override
        default void close() {
        }
    }

    private interface TransportConnection extends AutoCloseable {
        Map<String, FsEntry> listDir(String rel) throws Exception;

        FsEntry stat(String rel) throws Exception;

        ReadStream openRead(String rel) throws Exception;

        WriteStream openWrite(String rel) throws Exception;

        void rename(String src, String dst) throws Exception;

        void deleteFile(String rel) throws Exception;

        void deleteDir(String rel) throws Exception;

        void deleteTree(String rel) throws Exception;

        void createDir(String rel) throws Exception;

        void setModTime(String rel, Instant instant) throws Exception;

        default byte[] readAll(String rel) throws Exception {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            try (ReadStream in = openRead(rel)) {
                while (true) {
                    byte[] bytes = in.read(CHUNK_SIZE);
                    if (bytes.length == 0) {
                        return out.toByteArray();
                    }
                    out.write(bytes);
                }
            }
        }

        default void writeAll(String rel, byte[] bytes) throws Exception {
            try (WriteStream out = openWrite(rel)) {
                out.write(bytes);
            }
        }
    }

    private interface ReadStream extends AutoCloseable {
        byte[] read(int maxBytes) throws Exception;
    }

    private interface WriteStream extends AutoCloseable {
        void write(byte[] bytes) throws Exception;
    }

    private static final class LocalTransport implements Transport {
        private final Path root;

        private LocalTransport(Path root) {
            this.root = root.toAbsolutePath().normalize();
        }

        @Override
        public TransportConnection borrow() {
            return new LocalConnection(root);
        }
    }

    private static final class LocalConnection implements TransportConnection {
        private final Path root;

        private LocalConnection(Path root) {
            this.root = root;
        }

        @Override
        public Map<String, FsEntry> listDir(String rel) throws IOException {
            Path dir = path(rel);
            if (!Files.isDirectory(dir, LinkOption.NOFOLLOW_LINKS)) {
                throw new NotFoundException(rel);
            }
            Map<String, FsEntry> entries = new TreeMap<>();
            try (DirectoryStream<Path> stream = Files.newDirectoryStream(dir)) {
                for (Path child : stream) {
                    String name = child.getFileName().toString();
                    if (".kitchensync".equals(name)) {
                        continue;
                    }
                    Optional<FsEntry> entry = inspect(name, child);
                    entry.ifPresent(e -> entries.put(name, e));
                }
            }
            return entries;
        }

        @Override
        public FsEntry stat(String rel) throws IOException {
            Path p = path(rel);
            Optional<FsEntry> entry = inspect(basename(rel), p);
            if (entry.isEmpty()) {
                throw new NotFoundException(rel);
            }
            return entry.get();
        }

        @Override
        public ReadStream openRead(String rel) throws IOException {
            if (!Files.isRegularFile(path(rel), LinkOption.NOFOLLOW_LINKS)) {
                throw new NotFoundException(rel);
            }
            java.io.InputStream in = Files.newInputStream(path(rel));
            return new ReadStream() {
                @Override
                public byte[] read(int maxBytes) throws IOException {
                    byte[] buffer = in.readNBytes(maxBytes);
                    return buffer;
                }

                @Override
                public void close() throws IOException {
                    in.close();
                }
            };
        }

        @Override
        public WriteStream openWrite(String rel) throws IOException {
            Path p = path(rel);
            Files.createDirectories(p.getParent());
            java.io.OutputStream out = Files.newOutputStream(p);
            return new WriteStream() {
                @Override
                public void write(byte[] bytes) throws IOException {
                    out.write(bytes);
                }

                @Override
                public void close() throws IOException {
                    out.close();
                }
            };
        }

        @Override
        public void rename(String src, String dst) throws IOException {
            Path from = path(src);
            Path to = path(dst);
            Files.createDirectories(to.getParent());
            Files.move(from, to, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
        }

        @Override
        public void deleteFile(String rel) throws IOException {
            Files.deleteIfExists(path(rel));
        }

        @Override
        public void deleteDir(String rel) throws IOException {
            Files.deleteIfExists(path(rel));
        }

        @Override
        public void deleteTree(String rel) {
            deleteRecursively(path(rel));
        }

        @Override
        public void createDir(String rel) throws IOException {
            Files.createDirectories(path(rel));
        }

        @Override
        public void setModTime(String rel, Instant instant) throws IOException {
            Files.setLastModifiedTime(path(rel), FileTime.from(instant));
        }

        @Override
        public void close() {
        }

        private Path path(String rel) {
            if (rel == null || rel.isEmpty()) {
                return root;
            }
            return root.resolve(rel.replace('/', java.io.File.separatorChar)).normalize();
        }

        private Optional<FsEntry> inspect(String name, Path p) throws IOException {
            BasicFileAttributes attrs;
            try {
                attrs = Files.readAttributes(p, BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
            } catch (IOException ex) {
                return Optional.empty();
            }
            if (attrs.isSymbolicLink() || (!attrs.isRegularFile() && !attrs.isDirectory())) {
                return Optional.empty();
            }
            return Optional.of(new FsEntry(name, attrs.isDirectory(), attrs.lastModifiedTime().toInstant(),
                    attrs.isDirectory() ? -1L : attrs.size()));
        }
    }

    private static final class SftpTransport implements Transport {
        private final ParsedUrl url;
        private final SftpPools pools;
        private final Options options;

        private SftpTransport(ParsedUrl url, SftpPools pools, Options options) {
            this.url = url;
            this.pools = pools;
            this.options = options;
        }

        @Override
        public TransportConnection borrow() throws InterruptedException {
            return pools.acquire(url, options);
        }

        @Override
        public TransportConnection openListingConnection() throws InterruptedException {
            SftpPool listingPool = new SftpPool(new SftpPoolConfig(1,
                    SftpPools.intParam(url, "ct", options.connectTimeoutSeconds),
                    SftpPools.intParam(url, "ka", options.keepaliveSeconds)));
            ConnectionHandle listHandle = listingPool.acquire(SftpPools.sftpUri(url));
            return new SftpConnection(url.path(), listHandle, () -> {
                try {
                    listingPool.release(listHandle);
                } finally {
                    listingPool.shutdown();
                }
            });
        }
    }

    private static final class SftpPools {
        private final Map<String, SftpPool> pools = new HashMap<>();
        private final Map<String, Integer> inUse = new HashMap<>();
        private final Map<String, Integer> maxConnections = new HashMap<>();
        private final Logger log;

        private SftpPools(Logger log) {
            this.log = log;
        }

        SftpConnection acquire(ParsedUrl url, Options options) throws InterruptedException {
            String key = endpoint(url);
            SftpPool pool;
            synchronized (this) {
                pool = pools.computeIfAbsent(key, unused -> {
                    SftpPoolConfig config = new SftpPoolConfig(intParam(url, "mc", options.maxConnections),
                            intParam(url, "ct", options.connectTimeoutSeconds),
                            intParam(url, "ka", options.keepaliveSeconds));
                    maxConnections.put(key, config.maxConnections());
                    return new SftpPool(config);
                });
            }
            ConnectionHandle handle = pool.acquire(sftpUri(url));
            int count;
            int max;
            synchronized (this) {
                count = inUse.merge(key, 1, Integer::sum);
                max = maxConnections.get(key);
            }
            log.trace("endpoint=" + key + " connections=" + count + "/" + max);
            return new SftpConnection(url.path(), key, pool, handle, this, log);
        }

        void release(String key, SftpPool pool, ConnectionHandle handle) {
            int count;
            int max;
            synchronized (this) {
                count = Math.max(0, inUse.merge(key, -1, Integer::sum));
                max = maxConnections.get(key);
            }
            log.trace("endpoint=" + key + " connections=" + count + "/" + max);
            pool.release(handle);
        }

        synchronized void shutdown() {
            for (SftpPool pool : pools.values()) {
                pool.shutdown();
            }
            pools.clear();
        }

        static int intParam(ParsedUrl url, String key, int fallback) {
            String value = url.query().get(key);
            if (value == null) {
                return fallback;
            }
            try {
                return Integer.parseInt(value);
            } catch (NumberFormatException ex) {
                return fallback;
            }
        }

        private static String endpoint(ParsedUrl url) {
            return url.user() + "@" + url.host();
        }

        static String sftpUri(ParsedUrl url) {
            StringBuilder builder = new StringBuilder("sftp://");
            builder.append(url.user());
            if (url.password() != null) {
                builder.append(':').append(url.password());
            }
            builder.append('@').append(url.host());
            if (url.port() != null) {
                builder.append(':').append(url.port());
            }
            builder.append(url.path());
            return builder.toString();
        }
    }

    private static final class SftpConnection implements TransportConnection {
        private final String root;
        private final ConnectionHandle handle;
        private final Runnable release;
        private boolean closed;

        private SftpConnection(String root, String key, SftpPool pool, ConnectionHandle handle, SftpPools owner, Logger log) {
            this(root, handle, () -> owner.release(key, pool, handle));
        }

        private SftpConnection(String root, ConnectionHandle handle, Runnable release) {
            this.root = root;
            this.handle = handle;
            this.release = release;
        }

        @Override
        public Map<String, FsEntry> listDir(String rel) {
            Map<String, FsEntry> entries = new TreeMap<>();
            for (DirEntry entry : handle.listDir(path(rel))) {
                if (".kitchensync".equals(entry.name())) {
                    continue;
                }
                entries.put(entry.name(), new FsEntry(entry.name(), entry.isDir(), entry.modTime(), entry.byteSize()));
            }
            return entries;
        }

        @Override
        public FsEntry stat(String rel) {
            try {
                StatResult stat = handle.stat(path(rel));
                return new FsEntry(basename(rel), stat.isDir(), stat.modTime(), stat.byteSize());
            } catch (SftpNotFoundException ex) {
                throw new NotFoundException(rel, ex);
            }
        }

        @Override
        public ReadStream openRead(String rel) {
            sftp.protocol.ReadHandle read;
            try {
                read = handle.openRead(path(rel));
            } catch (SftpNotFoundException ex) {
                throw new NotFoundException(rel, ex);
            }
            return new ReadStream() {
                @Override
                public byte[] read(int maxBytes) {
                    byte[] bytes = handle.read(read, maxBytes);
                    return bytes == null ? new byte[0] : bytes;
                }

                @Override
                public void close() {
                    handle.closeRead(read);
                }
            };
        }

        @Override
        public WriteStream openWrite(String rel) {
            sftp.protocol.WriteHandle write = handle.openWrite(path(rel));
            return new WriteStream() {
                @Override
                public void write(byte[] bytes) {
                    handle.write(write, bytes);
                }

                @Override
                public void close() {
                    handle.closeWrite(write);
                }
            };
        }

        @Override
        public void rename(String src, String dst) {
            handle.rename(path(src), path(dst));
        }

        @Override
        public void deleteFile(String rel) {
            handle.deleteFile(path(rel));
        }

        @Override
        public void deleteDir(String rel) {
            handle.deleteDir(path(rel));
        }

        @Override
        public void deleteTree(String rel) {
            try {
                FsEntry entry = stat(rel);
                if (!entry.directory) {
                    deleteFile(rel);
                    return;
                }
                for (String name : listDir(rel).keySet()) {
                    deleteTree(child(rel, name));
                }
                deleteDir(rel);
            } catch (NotFoundException ignored) {
            }
        }

        @Override
        public void createDir(String rel) {
            handle.createDir(path(rel));
        }

        @Override
        public void setModTime(String rel, Instant instant) {
            handle.setModTime(path(rel), instant);
        }

        @Override
        public void close() {
            if (!closed) {
                closed = true;
                release.run();
            }
        }

        private String path(String rel) {
            if (rel == null || rel.isEmpty()) {
                return root;
            }
            if (root.endsWith("/")) {
                return root + rel;
            }
            return root + "/" + rel;
        }
    }

    private record FsEntry(String name, boolean directory, Instant modTime, long byteSize) {
    }

    private record ListingResult(Map<String, FsEntry> entries, Throwable error) {
        static ListingResult success(Map<String, FsEntry> entries) {
            return new ListingResult(entries, null);
        }

        static ListingResult failure(Throwable error) {
            return new ListingResult(Map.of(), error);
        }
    }

    private record DecisionContext(String rel, Decision decision, Map<Peer, Optional<SnapshotRecord>> records) {
    }

    private static final class Peer {
        private final String id;
        private final url.parser.Role explicitRole;
        private final ParsedUrl url;
        private final Transport transport;
        private final TransportConnection listingConnection;
        private final Map<String, Action> lastActions = new HashMap<>();
        private boolean snapshotExisted;
        private boolean subordinate;
        private Path snapshotPath;
        private SnapshotStore store;

        private Peer(String id, url.parser.Role explicitRole, ParsedUrl url, Transport transport,
                     TransportConnection listingConnection) {
            this.id = id;
            this.explicitRole = explicitRole;
            this.url = url;
            this.transport = transport;
            this.listingConnection = listingConnection;
        }

        boolean contributes() {
            return !subordinate;
        }

        decision.engine.Role decisionRole() {
            if (explicitRole == Role.CANON) {
                return decision.engine.Role.CANON;
            }
            return subordinate ? decision.engine.Role.SUBORDINATE : decision.engine.Role.CONTRIBUTING;
        }

        String identity() {
            return url.identity();
        }
    }

    private record Options(int maxConnections, int connectTimeoutSeconds, int keepaliveSeconds,
                           String verbosity, int tmpDays, int backupDays, int tombstoneDays) {
        static Options defaults() {
            return new Options(10, 30, 30, "info", 2, 90, 180);
        }
    }

    private record PeerSpec(url.parser.Role role, List<ParsedUrl> urls) {
        boolean canon() {
            return role == url.parser.Role.CANON;
        }
    }

    private static final class Cli {
        private final Options options;
        private final List<PeerSpec> peers;
        private final boolean help;
        private final String error;

        private Cli(Options options, List<PeerSpec> peers, boolean help, String error) {
            this.options = options;
            this.peers = peers;
            this.help = help;
            this.error = error;
        }

        static Cli parse(String[] args) {
            if (args.length == 0) {
                return new Cli(Options.defaults(), List.of(), true, null);
            }
            for (String arg : args) {
                if ("-h".equals(arg) || "--help".equals(arg) || "/?".equals(arg)) {
                    return new Cli(Options.defaults(), List.of(), true, null);
                }
            }

            Options defaults = Options.defaults();
            int mc = defaults.maxConnections;
            int ct = defaults.connectTimeoutSeconds;
            int ka = defaults.keepaliveSeconds;
            String vl = defaults.verbosity;
            int xd = defaults.tmpDays;
            int bd = defaults.backupDays;
            int td = defaults.tombstoneDays;
            List<String> peerArgs = new ArrayList<>();
            for (int i = 0; i < args.length; i++) {
                String arg = args[i];
                try {
                    switch (arg) {
                        case "--mc" -> mc = positive(next(args, ++i, arg), arg);
                        case "--ct" -> ct = positive(next(args, ++i, arg), arg);
                        case "--ka" -> ka = positive(next(args, ++i, arg), arg);
                        case "--xd" -> xd = positive(next(args, ++i, arg), arg);
                        case "--bd" -> bd = positive(next(args, ++i, arg), arg);
                        case "--td" -> td = positive(next(args, ++i, arg), arg);
                        case "-vl" -> {
                            vl = next(args, ++i, arg);
                            if (!Set.of("error", "info", "debug", "trace").contains(vl)) {
                                return invalid("invalid -vl value: " + vl);
                            }
                        }
                        default -> {
                            if (arg.startsWith("--")) {
                                return invalid("unrecognized flag: " + arg);
                            }
                            peerArgs.add(arg);
                        }
                    }
                } catch (IllegalArgumentException ex) {
                    return invalid(ex.getMessage());
                }
            }
            if (peerArgs.size() < 2) {
                return invalid("at least two peers are required");
            }
            String cwd = Path.of("").toAbsolutePath().normalize().toString();
            String user = System.getProperty("user.name", "");
            List<PeerSpec> parsedPeers = new ArrayList<>();
            for (String peerArg : peerArgs) {
                try {
                    TaggedGroup group = UrlParser.parse(peerArg, cwd, user);
                    parsedPeers.add(new PeerSpec(group.role(), group.urls()));
                } catch (RuntimeException ex) {
                    return invalid("invalid peer: " + peerArg);
                }
            }
            List<PeerSpec> peers = deduplicatePeers(parsedPeers);
            int canon = 0;
            for (PeerSpec peer : peers) {
                if (peer.role() == url.parser.Role.CANON) {
                    canon++;
                }
            }
            if (canon > 1) {
                return invalid("at most one canon peer is allowed");
            }
            return new Cli(new Options(mc, ct, ka, vl, xd, bd, td), peers, false, null);
        }

        private static List<PeerSpec> deduplicatePeers(List<PeerSpec> peers) {
            Map<String, PeerSpec> unique = new LinkedHashMap<>();
            for (PeerSpec peer : peers) {
                if (peer.urls().isEmpty()) {
                    continue;
                }
                String identity = peer.urls().getFirst().identity();
                PeerSpec existing = unique.get(identity);
                if (existing == null) {
                    unique.put(identity, peer);
                } else {
                    unique.put(identity, new PeerSpec(preferredRole(existing.role(), peer.role()), existing.urls()));
                }
            }
            return new ArrayList<>(unique.values());
        }

        private static url.parser.Role preferredRole(url.parser.Role first, url.parser.Role second) {
            if (first == url.parser.Role.CANON || second == url.parser.Role.CANON) {
                return url.parser.Role.CANON;
            }
            if (first == url.parser.Role.SUBORDINATE || second == url.parser.Role.SUBORDINATE) {
                return url.parser.Role.SUBORDINATE;
            }
            return url.parser.Role.NORMAL;
        }

        private static Cli invalid(String message) {
            return new Cli(Options.defaults(), List.of(), false, message);
        }

        private static String next(String[] args, int index, String flag) {
            if (index >= args.length) {
                throw new IllegalArgumentException("missing value for " + flag);
            }
            return args[index];
        }

        private static int positive(String text, String flag) {
            try {
                int value = Integer.parseInt(text);
                if (value <= 0) {
                    throw new NumberFormatException();
                }
                return value;
            } catch (NumberFormatException ex) {
                throw new IllegalArgumentException("invalid value for " + flag + ": " + text);
            }
        }
    }

    private static final class Logger {
        private final int level;

        private Logger(String verbosity) {
            this.level = switch (verbosity) {
                case "error" -> 0;
                case "debug" -> 2;
                case "trace" -> 3;
                default -> 1;
            };
        }

        void error(String message) {
            if (level >= 0) {
                System.out.println(message);
            }
        }

        void info(String message) {
            if (level >= 1) {
                System.out.println(message);
            }
        }

        void trace(String message) {
            if (level >= 3) {
                System.out.println(message);
            }
        }
    }

    private static class SyncException extends RuntimeException {
        SyncException(String message) {
            super(message);
        }

        SyncException(String message, Throwable cause) {
            super(message, cause);
        }
    }

    private static final class NotFoundException extends SyncException {
        NotFoundException(String message) {
            super(message);
        }

        NotFoundException(String message, Throwable cause) {
            super(message, cause);
        }
    }

    private static final class Help {
        private static final String TEXT = """
Usage: java -jar kitchensync.jar [options] <peer> <peer> [<peer>...]

Synchronize file trees across multiple peers.

Running with no arguments prints this help. See README.md for full docs.

Peers:
  /path or c:\\path                 Local path (same as file://)
  sftp://user@host/path            Remote over SSH
  sftp://user@host:port/path       Non-standard SSH port
  sftp://host/path                 Remote over SSH, current OS user
  sftp://user:password@host/path   Inline password (prefer SSH keys)

Prefix modifiers:
  +<peer>                          Canon — this peer's state wins all conflicts
  -<peer>                          Subordinate — overwritten to match the group

Fallback URLs (multiple paths to the same data):
  [url1,url2,...]                  Try in order, first that connects wins
  +[url1,url2,...]                 Canon peer with fallbacks
  -[url1,url2,...]                 Subordinate peer with fallbacks

Per-URL settings (query string, inside quotes):
  "sftp://host/path?mc=5"          Max connections for this URL
  "sftp://host/path?ct=60"         Connection timeout for this URL
  "sftp://host/path?ka=10"         SFTP idle keep-alive TTL for this URL
  "sftp://host/path?mc=5&ct=60"    Combine multiple

Options:
  -h, --help, /?                      Show this help
  --mc N             Max concurrent connections per URL (default: 10)
  --ct N             SSH handshake timeout in seconds (default: 30)
  --ka N             SFTP idle keep-alive TTL in seconds (default: 30)
  -vl LEVEL          Verbosity level: error, info, debug, trace (default: info)
  --xd N             Delete stale TMP staging after N days (default: 2)
  --bd N             Delete displaced files (BAK/) after N days (default: 90)
  --td N             Forget deletion records after N days (default: 180)

Quick start:
  java -jar kitchensync.jar +c:/photos sftp://user@host/photos      First sync (c: is canon)
  java -jar kitchensync.jar c:/photos sftp://host/photos            Bidirectional
  java -jar kitchensync.jar c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate
  java -jar kitchensync.jar c:/photos "sftp://user:p%40ss@host/photos"  Inline password

Canon (+) is required on first sync when no peer has snapshot history.
After the first sync, bidirectional sync works without canon.

Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.

Displaced files are recoverable from .kitchensync/BAK/ (kept for --bd days).
""";
    }
}
