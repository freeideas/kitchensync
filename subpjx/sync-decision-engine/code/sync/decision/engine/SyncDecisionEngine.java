package sync.decision.engine;

import java.time.Duration;
import java.time.Instant;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class SyncDecisionEngine {
    private static final Duration TOLERANCE = Duration.ofSeconds(5);

    private SyncDecisionEngine() {
    }

    public static EntryDecision decideEntry(DecisionInput input) {
        validate(input);

        if (input.peers().values().stream().noneMatch(SyncDecisionEngine::contributes)) {
            return new EntryDecision(AuthoritativeState.absent(), new LinkedHashMap<>(), new LinkedHashMap<>(), List.of(), true);
        }

        PeerId canon = canonPeer(input.peers());
        AuthoritativeState state = canon == null ? chooseWithoutCanon(input) : chooseCanon(input, canon);
        return conform(input, state);
    }

    public static EntryDecision decide_entry(DecisionInput input) {
        return decideEntry(input);
    }

    private static void validate(DecisionInput input) {
        if (input == null) {
            throw new InvalidInputException();
        }

        int canonCount = 0;
        for (PeerRole role : input.peers().values()) {
            if (role == null) {
                throw new InvalidInputException();
            }
            if (role == PeerRole.CANON) {
                canonCount++;
            }
        }
        if (canonCount > 1) {
            throw new InvalidInputException();
        }
        for (PeerId peer : input.liveEntries().keySet()) {
            if (!input.peers().containsKey(peer)) {
                throw new InvalidInputException();
            }
        }
        for (PeerId peer : input.snapshotRows().keySet()) {
            if (!input.peers().containsKey(peer)) {
                throw new InvalidInputException();
            }
        }
        for (LiveEntry entry : input.liveEntries().values()) {
            validateEntry(entry.kind(), entry.byteSize());
        }
        for (SnapshotRow row : input.snapshotRows().values()) {
            validateEntry(row.kind(), row.byteSize());
            if (row.deletedTime() != null && row.lastSeen() == null) {
                throw new InvalidInputException();
            }
        }
    }

    private static void validateEntry(EntryKind kind, long byteSize) {
        if (kind == EntryKind.FILE && byteSize < 0) {
            throw new InvalidInputException();
        }
        if (kind == EntryKind.DIRECTORY && byteSize != -1) {
            throw new InvalidInputException();
        }
    }

    private static boolean contributes(PeerRole role) {
        return role == PeerRole.CANON || role == PeerRole.NORMAL;
    }

    private static PeerId canonPeer(LinkedHashMap<PeerId, PeerRole> peers) {
        for (var entry : peers.entrySet()) {
            if (entry.getValue() == PeerRole.CANON) {
                return entry.getKey();
            }
        }
        return null;
    }

    private static AuthoritativeState chooseCanon(DecisionInput input, PeerId canon) {
        LiveEntry live = input.liveEntries().get(canon);
        if (live == null) {
            return AuthoritativeState.absent();
        }
        if (live.kind() == EntryKind.FILE) {
            return AuthoritativeState.file(canon, live.modTime(), live.byteSize());
        }
        return AuthoritativeState.directory(canon);
    }

    private static AuthoritativeState chooseWithoutCanon(DecisionInput input) {
        if (hasContributingLive(input, EntryKind.FILE)) {
            return chooseFile(input);
        }
        if (hasContributingLive(input, EntryKind.DIRECTORY)) {
            PeerId source = firstContributingLive(input, EntryKind.DIRECTORY);
            return AuthoritativeState.directory(source);
        }
        return AuthoritativeState.absent();
    }

    private static boolean hasContributingLive(DecisionInput input, EntryKind kind) {
        return firstContributingLive(input, kind) != null;
    }

    private static PeerId firstContributingLive(DecisionInput input, EntryKind kind) {
        for (var peer : input.peers().entrySet()) {
            LiveEntry live = input.liveEntries().get(peer.getKey());
            if (contributes(peer.getValue()) && live != null && live.kind() == kind) {
                return peer.getKey();
            }
        }
        return null;
    }

    private static AuthoritativeState chooseFile(DecisionInput input) {
        FileCandidate winner = null;
        Instant latestLiveFileTime = null;
        for (var peer : input.peers().entrySet()) {
            LiveEntry live = input.liveEntries().get(peer.getKey());
            if (contributes(peer.getValue()) && live != null && live.kind() == EntryKind.FILE) {
                SnapshotRow row = input.snapshotRows().get(peer.getKey());
                FileCandidate candidate = new FileCandidate(peer.getKey(), live.modTime(), live.byteSize(), isChangedFile(live, row));
                if (winner == null || fileBeats(candidate, winner)) {
                    winner = candidate;
                }
                if (latestLiveFileTime == null || live.modTime().isAfter(latestLiveFileTime)) {
                    latestLiveFileTime = live.modTime();
                }
            }
        }

        Instant deletion = latestDeletionEstimate(input, latestLiveFileTime);
        if (winner == null) {
            return AuthoritativeState.absent();
        }
        if (deletion != null && laterThanTolerance(deletion, latestLiveFileTime)) {
            return AuthoritativeState.absent();
        }
        return AuthoritativeState.file(winner.peer(), winner.modTime(), winner.byteSize());
    }

    private static boolean isChangedFile(LiveEntry live, SnapshotRow row) {
        if (row == null || row.kind() != EntryKind.FILE || row.deletedTime() != null) {
            return true;
        }
        return laterThanTolerance(live.modTime(), row.modTime()) || laterThanTolerance(row.modTime(), live.modTime());
    }

    private static boolean fileBeats(FileCandidate candidate, FileCandidate current) {
        if (candidate.changed() != current.changed()) {
            return candidate.changed();
        }
        if (laterThanTolerance(candidate.modTime(), current.modTime())) {
            return true;
        }
        if (laterThanTolerance(current.modTime(), candidate.modTime())) {
            return false;
        }
        return candidate.byteSize() > current.byteSize();
    }

    private static Instant latestDeletionEstimate(DecisionInput input, Instant latestLiveFileTime) {
        Instant latest = null;
        for (var peer : input.peers().entrySet()) {
            if (!contributes(peer.getValue())) {
                continue;
            }
            LiveEntry live = input.liveEntries().get(peer.getKey());
            if (live != null) {
                continue;
            }
            SnapshotRow row = input.snapshotRows().get(peer.getKey());
            if (row == null || row.kind() != EntryKind.FILE) {
                continue;
            }
            Instant estimate = null;
            if (row.deletedTime() != null) {
                estimate = row.deletedTime();
            } else if (latestLiveFileTime != null && row.lastSeen() != null && laterThanTolerance(row.lastSeen(), latestLiveFileTime)) {
                estimate = row.lastSeen();
            }
            if (estimate != null && (latest == null || estimate.isAfter(latest))) {
                latest = estimate;
            }
        }
        return latest;
    }

    private static boolean laterThanTolerance(Instant left, Instant right) {
        return Duration.between(right, left).compareTo(TOLERANCE) > 0;
    }

    private static EntryDecision conform(DecisionInput input, AuthoritativeState state) {
        LinkedHashMap<PeerId, List<FilesystemEffect>> filesystem = new LinkedHashMap<>();
        LinkedHashMap<PeerId, List<SnapshotEffect>> snapshot = new LinkedHashMap<>();
        List<PeerId> recurse = state.kind() == AuthoritativeKind.DIRECTORY ? new ArrayList<>() : List.of();

        for (PeerId peer : input.peers().keySet()) {
            LiveEntry live = input.liveEntries().get(peer);
            SnapshotRow row = input.snapshotRows().get(peer);
            filesystem.put(peer, filesystemEffects(state, live));
            snapshot.put(peer, snapshotEffects(state, live, row));
            if (state.kind() == AuthoritativeKind.DIRECTORY) {
                recurse.add(peer);
            }
        }

        return new EntryDecision(state, filesystem, snapshot, recurse, false);
    }

    private static List<FilesystemEffect> filesystemEffects(AuthoritativeState state, LiveEntry live) {
        return switch (state.kind()) {
            case ABSENT -> live == null ? List.of(FilesystemEffect.KEEP) : List.of(FilesystemEffect.DISPLACE);
            case FILE -> fileFilesystemEffects(state, live);
            case DIRECTORY -> directoryFilesystemEffects(live);
        };
    }

    private static List<FilesystemEffect> fileFilesystemEffects(AuthoritativeState state, LiveEntry live) {
        if (matchesFile(state, live)) {
            return List.of(FilesystemEffect.KEEP);
        }
        if (live != null && live.kind() == EntryKind.DIRECTORY) {
            return List.of(FilesystemEffect.DISPLACE, FilesystemEffect.COPY_FILE);
        }
        return List.of(FilesystemEffect.COPY_FILE);
    }

    private static List<FilesystemEffect> directoryFilesystemEffects(LiveEntry live) {
        if (live != null && live.kind() == EntryKind.DIRECTORY) {
            return List.of(FilesystemEffect.KEEP);
        }
        if (live != null) {
            return List.of(FilesystemEffect.DISPLACE, FilesystemEffect.CREATE_DIRECTORY);
        }
        return List.of(FilesystemEffect.CREATE_DIRECTORY);
    }

    private static List<SnapshotEffect> snapshotEffects(AuthoritativeState state, LiveEntry live, SnapshotRow row) {
        return switch (state.kind()) {
            case ABSENT -> absentSnapshotEffects(live, row);
            case FILE -> fileSnapshotEffects(state, live);
            case DIRECTORY -> directorySnapshotEffects(live);
        };
    }

    private static List<SnapshotEffect> absentSnapshotEffects(LiveEntry live, SnapshotRow row) {
        if (live != null) {
            return List.of(SnapshotEffect.MARK_DISPLACED);
        }
        if (row != null && row.deletedTime() == null) {
            return List.of(SnapshotEffect.MARK_ABSENT);
        }
        return List.of(SnapshotEffect.NO_SNAPSHOT_CHANGE);
    }

    private static List<SnapshotEffect> fileSnapshotEffects(AuthoritativeState state, LiveEntry live) {
        if (matchesFile(state, live)) {
            return List.of(SnapshotEffect.CONFIRM_PRESENT);
        }
        if (live != null && live.kind() == EntryKind.DIRECTORY) {
            return List.of(SnapshotEffect.MARK_DISPLACED, SnapshotEffect.COPY_PENDING);
        }
        return List.of(SnapshotEffect.COPY_PENDING);
    }

    private static List<SnapshotEffect> directorySnapshotEffects(LiveEntry live) {
        if (live != null && live.kind() == EntryKind.DIRECTORY) {
            return List.of(SnapshotEffect.CONFIRM_PRESENT);
        }
        if (live != null) {
            return List.of(SnapshotEffect.MARK_DISPLACED, SnapshotEffect.CREATE_DIRECTORY_CONFIRMED);
        }
        return List.of(SnapshotEffect.CREATE_DIRECTORY_CONFIRMED);
    }

    private static boolean matchesFile(AuthoritativeState state, LiveEntry live) {
        return live != null
                && live.kind() == EntryKind.FILE
                && live.byteSize() == state.byteSize()
                && !laterThanTolerance(live.modTime(), state.modTime())
                && !laterThanTolerance(state.modTime(), live.modTime());
    }

    private record FileCandidate(PeerId peer, Instant modTime, long byteSize, boolean changed) {
    }
}
