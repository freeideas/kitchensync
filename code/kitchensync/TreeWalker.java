package kitchensync;

import java.time.Instant;
import java.time.temporal.ChronoUnit;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;

import gitignore.matcher.EntryKind;
import gitignore.matcher.IgnoreMatcher;
import gitignore.matcher.IgnoreOptions;
import gitignore.matcher.PathEntry;
import gitignore.matcher.PatternLayer;
import snapshot.database.EntryMetadata;
import snapshot.database.SnapshotDatabase;
import snapshot.database.SnapshotTime;
import sync.decision.engine.AuthoritativeKind;
import sync.decision.engine.DecisionInput;
import sync.decision.engine.EntryDecision;
import sync.decision.engine.FilesystemEffect;
import sync.decision.engine.LiveEntry;
import sync.decision.engine.PeerId;
import sync.decision.engine.PeerRole;
import sync.decision.engine.SnapshotEffect;
import sync.decision.engine.SyncDecisionEngine;

final class TreeWalker {
    private final ExecutorService executor;
    private final Logger logger;
    private final TimeUtil times;
    private final TransferManager transfers;
    private final RunOptions options;

    TreeWalker(ExecutorService executor, Logger logger, TimeUtil times, TransferManager transfers, RunOptions options) {
        this.executor = executor;
        this.logger = logger;
        this.times = times;
        this.transfers = transfers;
        this.options = options;
    }

    void walk(List<Peer> peers) {
        syncDirectory(peers, "", IgnoreMatcher.empty(IgnoreOptions.defaults()));
    }

    private void syncDirectory(List<Peer> peers, String dir, IgnoreMatcher matcher) {
        Map<Peer, CompletableFuture<List<EntryInfo>>> futures = new LinkedHashMap<>();
        for (Peer peer : peers) {
            futures.put(peer, CompletableFuture.supplyAsync(() -> list(peer, dir), executor));
        }
        Map<Peer, Map<String, EntryInfo>> listings = new LinkedHashMap<>();
        for (Map.Entry<Peer, CompletableFuture<List<EntryInfo>>> item : futures.entrySet()) {
            try {
                Map<String, EntryInfo> byName = new LinkedHashMap<>();
                for (EntryInfo entry : item.getValue().join()) {
                    byName.put(entry.name(), entry);
                }
                listings.put(item.getKey(), byName);
            } catch (RuntimeException ex) {
                logger.error("listing failed for " + item.getKey().url.normalized() + " at " + dir
                        + ", excluding from this subtree");
            }
        }
        List<Peer> active = listings.keySet().stream().toList();
        if (active.stream().noneMatch(p -> !p.subordinate())) {
            return;
        }

        Set<String> names = new LinkedHashSet<>();
        for (Map<String, EntryInfo> listing : listings.values()) {
            names.addAll(listing.keySet());
        }
        names.remove(".kitchensync");

        IgnoreMatcher currentMatcher = matcher;
        if (names.contains(".syncignore")) {
            EntryDecision ignoreDecision = decide(active, listings, dir, ".syncignore");
            applyDecision(active, listings, dir, ".syncignore", ignoreDecision);
            if (ignoreDecision.authoritativeState().kind() == AuthoritativeKind.FILE) {
                Peer source = peer(active, ignoreDecision.authoritativeState().sourcePeer()).orElse(null);
                if (source != null) {
                    try {
                        String text = readText(source.transport, PathUtil.child(dir, ".syncignore"));
                        currentMatcher = matcher.extend(new PatternLayer(dir, text, ".syncignore"));
                    } catch (Exception ex) {
                        logger.error("failed to read .syncignore at " + dir);
                    }
                }
            }
            names.remove(".syncignore");
        }

        List<String> filtered = new ArrayList<>();
        for (String name : names) {
            EntryInfo entry = firstEntry(listings, name);
            if (entry == null) {
                continue;
            }
            String relative = PathUtil.child(dir, name);
            if (name.equals(".kitchensync")) {
                continue;
            }
            EntryKind kind = entry.directory() ? EntryKind.directory : EntryKind.regular_file;
            if (!currentMatcher.match(new PathEntry(relative, kind)).ignored()) {
                filtered.add(name);
            }
        }
        filtered.sort(Comparator.naturalOrder());
        List<String> dirsToRecurse = new ArrayList<>();
        Map<String, List<Peer>> recursePeers = new LinkedHashMap<>();
        for (String name : filtered) {
            EntryDecision decision = decide(active, listings, dir, name);
            applyDecision(active, listings, dir, name, decision);
            if (decision.authoritativeState().kind() == AuthoritativeKind.DIRECTORY) {
                List<Peer> keepers = new ArrayList<>();
                for (PeerId id : decision.recursePeers()) {
                    peer(active, id).ifPresent(keepers::add);
                }
                if (!keepers.isEmpty()) {
                    String child = PathUtil.child(dir, name);
                    dirsToRecurse.add(child);
                    recursePeers.put(child, keepers);
                }
            }
        }
        cleanupMetadata(active, dir);
        for (String child : dirsToRecurse) {
            syncDirectory(recursePeers.get(child), child, currentMatcher);
        }
    }

    private List<EntryInfo> list(Peer peer, String dir) {
        try {
            return peer.transport.listDir(dir);
        } catch (TransportException ex) {
            throw new RuntimeException(ex);
        }
    }

    private EntryDecision decide(List<Peer> active, Map<Peer, Map<String, EntryInfo>> listings, String dir, String name) {
        String path = PathUtil.child(dir, name);
        LinkedHashMap<PeerId, PeerRole> roles = new LinkedHashMap<>();
        Map<PeerId, LiveEntry> live = new LinkedHashMap<>();
        Map<PeerId, sync.decision.engine.SnapshotRow> snapshots = new LinkedHashMap<>();
        for (Peer peer : active) {
            roles.put(peer.id, peer.role);
            EntryInfo entry = listings.get(peer).get(name);
            if (entry != null) {
                live.put(peer.id, new LiveEntry(entry.directory() ? sync.decision.engine.EntryKind.DIRECTORY
                        : sync.decision.engine.EntryKind.FILE, entry.modTime(), entry.byteSize()));
            }
            synchronized (peer.snapshot) {
                peer.snapshot.lookup(path).ifPresent(row -> snapshots.put(peer.id, toDecisionRow(row)));
            }
        }
        return SyncDecisionEngine.decideEntry(new DecisionInput(path, roles, live, snapshots));
    }

    private void applyDecision(List<Peer> active, Map<Peer, Map<String, EntryInfo>> listings, String dir, String name,
            EntryDecision decision) {
        String path = PathUtil.child(dir, name);
        EntryInfo winning = winningEntry(active, listings, name, decision);
        boolean loggedCopy = false;
        boolean loggedDelete = false;
        for (Peer peer : active) {
            List<SnapshotEffect> snapshotEffects = decision.snapshotEffects().getOrDefault(peer.id, List.of());
            for (SnapshotEffect effect : snapshotEffects) {
                applySnapshot(peer, path, winning, effect);
            }
        }
        for (Peer peer : active) {
            List<FilesystemEffect> effects = decision.filesystemEffects().getOrDefault(peer.id, List.of());
            for (FilesystemEffect effect : effects) {
                try {
                    if (effect == FilesystemEffect.CREATE_DIRECTORY) {
                        peer.transport.createDir(path);
                        recordCreatedDirectory(peer, path);
                    } else if (effect == FilesystemEffect.DISPLACE) {
                        displace(peer, path);
                        if (!loggedDelete) {
                            logger.info("X " + path);
                            loggedDelete = true;
                        }
                    } else if (effect == FilesystemEffect.COPY_FILE && winning != null) {
                        Peer source = peer(active, decision.authoritativeState().sourcePeer()).orElse(null);
                        if (source != null) {
                            EntryInfo current = listings.get(peer).get(name);
                            if (current != null && !current.directory() && !loggedDelete) {
                                logger.info("X " + path);
                                loggedDelete = true;
                            }
                            transfers.enqueue(source, peer, path, winning);
                            if (!loggedCopy) {
                                logger.info("C " + path);
                                loggedCopy = true;
                            }
                        }
                    }
                } catch (TransportException ex) {
                    logger.error("operation failed for " + path);
                }
            }
        }
    }

    private void applySnapshot(Peer peer, String path, EntryInfo winning, SnapshotEffect effect) {
        if (effect == SnapshotEffect.NO_SNAPSHOT_CHANGE) {
            return;
        }
        try {
            switch (effect) {
                case CONFIRM_PRESENT -> {
                    EntryInfo live = peer.transport.stat(path);
                    synchronized (peer.snapshot) {
                        if (!live.directory()) {
                            markOldDirectorySubtreeDisplaced(peer.snapshot, path);
                        }
                        peer.snapshot.record_present(path, metadata(live), times.nextSnapshotTime());
                    }
                }
                case COPY_PENDING -> {
                    if (winning != null) {
                        synchronized (peer.snapshot) {
                            if (!winning.directory()) {
                                markOldDirectorySubtreeDisplaced(peer.snapshot, path);
                            }
                            peer.snapshot.record_copy_pending(path, metadata(winning));
                        }
                    }
                }
                case MARK_ABSENT -> {
                    synchronized (peer.snapshot) {
                        if (peer.snapshot.lookup(path)
                                .map(row -> row.kind() == snapshot.database.EntryKind.DIRECTORY)
                                .orElse(false)) {
                            peer.snapshot.mark_displaced(path);
                        } else {
                            peer.snapshot.mark_absent(path);
                        }
                    }
                }
                case MARK_DISPLACED -> {
                    synchronized (peer.snapshot) {
                        peer.snapshot.mark_displaced(path);
                    }
                }
                default -> {
                }
            }
        } catch (Exception ex) {
            logger.error("snapshot update failed for " + path);
        }
    }

    private void recordCreatedDirectory(Peer peer, String path) {
        try {
            EntryInfo live = peer.transport.stat(path);
            synchronized (peer.snapshot) {
                peer.snapshot.record_present(path, metadata(live), times.nextSnapshotTime());
            }
        } catch (Exception ex) {
            logger.error("snapshot update failed for " + path);
        }
    }

    private static void markOldDirectorySubtreeDisplaced(SnapshotDatabase db, String path) {
        if (db.lookup(path)
                .map(row -> row.kind() == snapshot.database.EntryKind.DIRECTORY && row.deleted_time().isEmpty())
                .orElse(false)) {
            db.mark_displaced(path);
        }
    }

    private EntryInfo winningEntry(List<Peer> active, Map<Peer, Map<String, EntryInfo>> listings, String name,
            EntryDecision decision) {
        PeerId source = decision.authoritativeState().sourcePeer();
        if (source == null) {
            return null;
        }
        return peer(active, source).map(p -> listings.get(p).get(name)).orElse(null);
    }

    private EntryMetadata metadata(EntryInfo entry) {
        return new EntryMetadata(entry.directory() ? snapshot.database.EntryKind.DIRECTORY : snapshot.database.EntryKind.FILE,
                TimeUtil.snapshotTime(entry.modTime()), entry.byteSize());
    }

    private static sync.decision.engine.SnapshotRow toDecisionRow(snapshot.database.SnapshotRow row) {
        return new sync.decision.engine.SnapshotRow(
                row.kind() == snapshot.database.EntryKind.DIRECTORY ? sync.decision.engine.EntryKind.DIRECTORY
                        : sync.decision.engine.EntryKind.FILE,
                TimeUtil.instant(row.mod_time()),
                row.byte_size(),
                row.last_seen().map(TimeUtil::instant).orElse(null),
                row.deleted_time().map(TimeUtil::instant).orElse(null));
    }

    private void displace(Peer peer, String path) throws TransportException {
        String parent = PathUtil.parent(path);
        String basename = PathUtil.basename(path);
        String bakDir = PathUtil.child(parent, ".kitchensync/BAK/" + times.nextText());
        peer.transport.createDir(bakDir);
        peer.transport.rename(path, PathUtil.child(bakDir, basename));
    }

    private void cleanupMetadata(List<Peer> peers, String dir) {
        for (Peer peer : peers) {
            cleanup(peer.transport, PathUtil.child(dir, ".kitchensync/BAK"), options.bakRetentionDays);
            cleanup(peer.transport, PathUtil.child(dir, ".kitchensync/TMP"), options.tmpRetentionDays);
        }
    }

    private void cleanup(Transport transport, String path, int retentionDays) {
        try {
            Instant cutoff = Instant.now().minus(retentionDays, ChronoUnit.DAYS);
            for (EntryInfo entry : transport.listDir(path)) {
                if (entry.directory() && older(entry.name(), cutoff)) {
                    deleteRecursive(transport, PathUtil.child(path, entry.name()));
                }
            }
        } catch (TransportException ignored) {
        }
    }

    private void deleteRecursive(Transport transport, String path) throws TransportException {
        EntryInfo stat = transport.stat(path);
        if (!stat.directory()) {
            transport.deleteFile(path);
            return;
        }
        for (EntryInfo child : transport.listDir(path)) {
            deleteRecursive(transport, PathUtil.child(path, child.name()));
        }
        transport.deleteDir(path);
    }

    private boolean older(String timestamp, Instant cutoff) {
        try {
            return TimeUtil.instant(timestamp).isBefore(cutoff);
        } catch (RuntimeException ex) {
            return false;
        }
    }

    private static EntryInfo firstEntry(Map<Peer, Map<String, EntryInfo>> listings, String name) {
        for (Map<String, EntryInfo> listing : listings.values()) {
            EntryInfo entry = listing.get(name);
            if (entry != null) {
                return entry;
            }
        }
        return null;
    }

    private static Optional<Peer> peer(List<Peer> peers, PeerId id) {
        return peers.stream().filter(peer -> peer.id.equals(id)).findFirst();
    }

    private static String readText(Transport transport, String path) throws Exception {
        StringBuilder out = new StringBuilder();
        try (ReadToken read = transport.openRead(path)) {
            while (true) {
                byte[] chunk = transport.read(read, 64 * 1024);
                if (chunk.length == 0) {
                    break;
                }
                out.append(new String(chunk, java.nio.charset.StandardCharsets.UTF_8));
            }
        }
        return out.toString();
    }
}
