package decision.engine;

import java.time.Duration;
import java.util.Map;
import java.util.Objects;
import java.util.TreeMap;
import java.util.TreeSet;

public final class DecisionEngine {
    public static final long DEFAULT_TOLERANCE_SECONDS = 5L;
    public static final long DIRECTORY_BYTE_SIZE = -1L;

    private DecisionEngine() {
    }

    public static Decision decide_entry(
            Map<String, Role> roles,
            Map<String, Observation> observations,
            Map<String, HistoryRecord> histories,
            long toleranceSeconds) {
        return decideEntry(roles, observations, histories, toleranceSeconds);
    }

    public static Decision decide_entry(
            Map<String, Role> roles,
            Map<String, Observation> observations,
            Map<String, HistoryRecord> histories) {
        return decideEntry(roles, observations, histories);
    }

    public static Decision decideEntry(
            Map<String, Role> roles,
            Map<String, Observation> observations,
            Map<String, HistoryRecord> histories) {
        return decideEntry(roles, observations, histories, DEFAULT_TOLERANCE_SECONDS);
    }

    public static Decision decideEntry(
            Map<String, Role> roles,
            Map<String, Observation> observations,
            Map<String, HistoryRecord> histories,
            Duration tolerance) {
        return decideEntry(roles, observations, histories, tolerance.toSeconds());
    }

    public static Decision decideEntry(
            Map<String, Role> roles,
            Map<String, Observation> observations,
            Map<String, HistoryRecord> histories,
            long toleranceSeconds) {
        Objects.requireNonNull(roles, "roles");
        Objects.requireNonNull(observations, "observations");
        Objects.requireNonNull(histories, "histories");
        if (toleranceSeconds < 0) {
            throw new IllegalArgumentException("tolerance must be non-negative");
        }

        TreeSet<String> participants = new TreeSet<>();
        participants.addAll(roles.keySet());
        participants.addAll(observations.keySet());
        for (String participant : participants) {
            if (!roles.containsKey(participant)) {
                throw new IllegalArgumentException("missing role for participant: " + participant);
            }
            if (!observations.containsKey(participant)) {
                throw new IllegalArgumentException("missing observation for participant: " + participant);
            }
        }

        TreeMap<String, Classification> classifications = classify(participants, observations, histories, toleranceSeconds);
        String canon = findCanon(roles);
        DecisionState state = canon == null
                ? decideWithoutCanon(participants, roles, observations, histories, classifications, toleranceSeconds)
                : decideFromCanon(canon, observations.get(canon), histories.get(canon));
        TreeMap<String, Action> actions = canon == null && allContributingParticipantsUnchanged(participants, roles, classifications)
                ? allNoOp(participants)
                : reconcile(participants, observations, state, toleranceSeconds, canon != null);
        return new Decision(
                state.entryKind,
                state.winningModTime,
                state.winningByteSize,
                state.winningSource,
                actions,
                classifications);
    }

    private static boolean allContributingParticipantsUnchanged(
            TreeSet<String> participants,
            Map<String, Role> roles,
            Map<String, Classification> classifications) {
        boolean sawContributing = false;
        for (String participant : participants) {
            if (roles.get(participant) != Role.CONTRIBUTING) {
                continue;
            }
            sawContributing = true;
            if (classifications.get(participant) != Classification.UNCHANGED) {
                return false;
            }
        }
        return sawContributing;
    }

    private static TreeMap<String, Action> allNoOp(TreeSet<String> participants) {
        TreeMap<String, Action> actions = new TreeMap<>();
        for (String participant : participants) {
            actions.put(participant, Action.noOp());
        }
        return actions;
    }

    private static TreeMap<String, Classification> classify(
            TreeSet<String> participants,
            Map<String, Observation> observations,
            Map<String, HistoryRecord> histories,
            long toleranceSeconds) {
        TreeMap<String, Classification> classifications = new TreeMap<>();
        for (String participant : participants) {
            Observation observation = observations.get(participant);
            HistoryRecord history = histories.get(participant);
            classifications.put(participant, classifyOne(observation, history, toleranceSeconds));
        }
        return classifications;
    }

    private static Classification classifyOne(Observation observation, HistoryRecord history, long toleranceSeconds) {
        if (observation.kind() == Observation.Kind.ABSENT) {
            if (history == null) {
                return Classification.NO_OPINION;
            }
            return history.tombstone() ? Classification.DELETED : Classification.ABSENT_UNCONFIRMED;
        }
        if (history == null) {
            return Classification.NEW;
        }
        if (history.tombstone()) {
            return Classification.RESURRECTED;
        }
        if (observation.kind() == Observation.Kind.DIRECTORY) {
            return Classification.UNCHANGED;
        }
        return close(observation.requireModTime(), history.modTime(), toleranceSeconds)
                ? Classification.UNCHANGED
                : Classification.MODIFIED;
    }

    private static String findCanon(Map<String, Role> roles) {
        String canon = null;
        for (Map.Entry<String, Role> entry : roles.entrySet()) {
            if (entry.getValue() == Role.CANON) {
                if (canon != null) {
                    throw new IllegalArgumentException("more than one canon participant");
                }
                canon = entry.getKey();
            }
        }
        return canon;
    }

    private static DecisionState decideFromCanon(String canon, Observation observation, HistoryRecord history) {
        return switch (observation.kind()) {
            case FILE -> DecisionState.file(
                    observation.requireModTime(),
                    observation.requireByteSize(),
                    canon);
            case DIRECTORY -> DecisionState.directory(directoryModTime(history));
            case ABSENT -> DecisionState.none();
        };
    }

    private static DecisionState decideWithoutCanon(
            TreeSet<String> participants,
            Map<String, Role> roles,
            Map<String, Observation> observations,
            Map<String, HistoryRecord> histories,
            Map<String, Classification> classifications,
            long toleranceSeconds) {
        TreeSet<String> voters = new TreeSet<>();
        for (String participant : participants) {
            if (roles.get(participant) == Role.CONTRIBUTING) {
                voters.add(participant);
            }
        }

        boolean hasFile = false;
        boolean hasDirectory = false;
        for (String voter : voters) {
            Observation.Kind kind = observations.get(voter).kind();
            hasFile |= kind == Observation.Kind.FILE;
            hasDirectory |= kind == Observation.Kind.DIRECTORY;
        }

        if (hasFile) {
            return decideFile(voters, observations, histories, classifications, toleranceSeconds);
        }
        if (hasDirectory) {
            return DecisionState.directory(selectDirectoryModTime(voters, observations, histories));
        }
        return DecisionState.none();
    }

    private static DecisionState decideFile(
            TreeSet<String> voters,
            Map<String, Observation> observations,
            Map<String, HistoryRecord> histories,
            Map<String, Classification> classifications,
            long toleranceSeconds) {
        long maxLiveModTime = Long.MIN_VALUE;
        for (String voter : voters) {
            Observation observation = observations.get(voter);
            if (observation.kind() == Observation.Kind.FILE) {
                maxLiveModTime = Math.max(maxLiveModTime, observation.requireModTime());
            }
        }
        if (maxLiveModTime == Long.MIN_VALUE) {
            return DecisionState.none();
        }

        Long deletionEstimate = null;
        for (String voter : voters) {
            Classification classification = classifications.get(voter);
            HistoryRecord history = histories.get(voter);
            if (classification == Classification.DELETED) {
                deletionEstimate = maxNullable(deletionEstimate, history.deletedTime());
            } else if (classification == Classification.ABSENT_UNCONFIRMED
                    && history.lastSeen() != null
                    && history.lastSeen() - maxLiveModTime > toleranceSeconds) {
                deletionEstimate = maxNullable(deletionEstimate, history.lastSeen());
            }
        }
        return chooseSurvivingFile(voters, observations, toleranceSeconds, maxLiveModTime, deletionEstimate);
    }

    private static Long maxNullable(Long current, long candidate) {
        return current == null || candidate > current ? candidate : current;
    }

    private static DecisionState chooseSurvivingFile(
            TreeSet<String> voters,
            Map<String, Observation> observations,
            long toleranceSeconds,
            long maxLiveModTime,
            Long deletionEstimate) {
        if (deletionEstimate != null && deletionEstimate - maxLiveModTime > toleranceSeconds) {
            return DecisionState.none();
        }

        String winner = null;
        long winningByteSize = Long.MIN_VALUE;
        long winningModTime = Long.MIN_VALUE;
        for (String voter : voters) {
            Observation observation = observations.get(voter);
            if (observation.kind() != Observation.Kind.FILE) {
                continue;
            }
            long modTime = observation.requireModTime();
            if (maxLiveModTime - modTime > toleranceSeconds) {
                continue;
            }
            long byteSize = observation.requireByteSize();
            if (winner == null || byteSize > winningByteSize || (byteSize == winningByteSize && modTime > winningModTime)) {
                winner = voter;
                winningByteSize = byteSize;
                winningModTime = modTime;
            }
        }
        return DecisionState.file(winningModTime, winningByteSize, winner);
    }

    private static long selectDirectoryModTime(
            TreeSet<String> voters,
            Map<String, Observation> observations,
            Map<String, HistoryRecord> histories) {
        for (String voter : voters) {
            if (observations.get(voter).kind() == Observation.Kind.DIRECTORY) {
                return directoryModTime(histories.get(voter));
            }
        }
        return 0L;
    }

    private static long directoryModTime(HistoryRecord history) {
        return history == null ? 0L : history.modTime();
    }

    private static TreeMap<String, Action> reconcile(
            TreeSet<String> participants,
            Map<String, Observation> observations,
            DecisionState state,
            long toleranceSeconds,
            boolean canonDecision) {
        TreeMap<String, Action> actions = new TreeMap<>();
        for (String participant : participants) {
            Observation observation = observations.get(participant);
            actions.put(participant, actionFor(observation, state, toleranceSeconds, canonDecision));
        }
        return actions;
    }

    private static Action actionFor(Observation observation, DecisionState state, long toleranceSeconds, boolean canonDecision) {
        return switch (state.entryKind) {
            case FILE -> actionForFile(observation, state, toleranceSeconds, canonDecision);
            case DIRECTORY -> switch (observation.kind()) {
                case DIRECTORY -> Action.noOp();
                case ABSENT -> Action.createDirectory();
                case FILE -> Action.displace();
            };
            case NONE -> observation.kind() == Observation.Kind.ABSENT ? Action.noOp() : Action.displace();
        };
    }

    private static Action actionForFile(
            Observation observation,
            DecisionState state,
            long toleranceSeconds,
            boolean canonDecision) {
        if (observation.kind() == Observation.Kind.FILE
                && observation.requireByteSize() == state.winningByteSize
                && close(observation.requireModTime(), state.winningModTime, toleranceSeconds)) {
            return Action.noOp();
        }
        if (canonDecision || observation.kind() == Observation.Kind.ABSENT) {
            return Action.receiveFile(state.winningSource);
        }
        return Action.displace();
    }

    private static boolean close(long left, long right, long toleranceSeconds) {
        return Math.abs(left - right) <= toleranceSeconds;
    }

    private static final class DecisionState {
        private final EntryKind entryKind;
        private final Long winningModTime;
        private final Long winningByteSize;
        private final String winningSource;

        private DecisionState(EntryKind entryKind, Long winningModTime, Long winningByteSize, String winningSource) {
            this.entryKind = entryKind;
            this.winningModTime = winningModTime;
            this.winningByteSize = winningByteSize;
            this.winningSource = winningSource;
        }

        private static DecisionState file(long modTime, long byteSize, String source) {
            return new DecisionState(EntryKind.FILE, modTime, byteSize, source);
        }

        private static DecisionState directory(long modTime) {
            return new DecisionState(EntryKind.DIRECTORY, modTime, DIRECTORY_BYTE_SIZE, null);
        }

        private static DecisionState none() {
            return new DecisionState(EntryKind.NONE, null, null, null);
        }
    }
}
