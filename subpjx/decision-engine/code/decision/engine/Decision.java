package decision.engine;

import java.util.Map;
import java.util.Objects;
import java.util.TreeMap;

public final class Decision {
    private final EntryKind entryKind;
    private final Long winningModTime;
    private final Long winningByteSize;
    private final String winningSource;
    private final Map<String, Action> actions;
    private final Map<String, Classification> classifications;

    public Decision(
            EntryKind entryKind,
            Long winningModTime,
            Long winningByteSize,
            String winningSource,
            Map<String, Action> actions,
            Map<String, Classification> classifications) {
        this.entryKind = entryKind;
        this.winningModTime = winningModTime;
        this.winningByteSize = winningByteSize;
        this.winningSource = winningSource;
        this.actions = Map.copyOf(new TreeMap<>(actions));
        this.classifications = Map.copyOf(new TreeMap<>(classifications));
    }

    public EntryKind entryKind() {
        return entryKind;
    }

    public EntryKind getEntryKind() {
        return entryKind;
    }

    public Long winningModTime() {
        return winningModTime;
    }

    public Long getWinningModTime() {
        return winningModTime;
    }

    public Long winningByteSize() {
        return winningByteSize;
    }

    public Long getWinningByteSize() {
        return winningByteSize;
    }

    public String winningSource() {
        return winningSource;
    }

    public String getWinningSource() {
        return winningSource;
    }

    public Map<String, Action> actions() {
        return actions;
    }

    public Map<String, Action> getActions() {
        return actions;
    }

    public Map<String, Classification> classifications() {
        return classifications;
    }

    public Map<String, Classification> getClassifications() {
        return classifications;
    }

    @Override
    public boolean equals(Object other) {
        if (this == other) {
            return true;
        }
        if (!(other instanceof Decision decision)) {
            return false;
        }
        return entryKind == decision.entryKind
                && Objects.equals(winningModTime, decision.winningModTime)
                && Objects.equals(winningByteSize, decision.winningByteSize)
                && Objects.equals(winningSource, decision.winningSource)
                && actions.equals(decision.actions)
                && classifications.equals(decision.classifications);
    }

    @Override
    public int hashCode() {
        return Objects.hash(entryKind, winningModTime, winningByteSize, winningSource, actions, classifications);
    }

    @Override
    public String toString() {
        return "Decision[entryKind=" + entryKind
                + ", winningModTime=" + winningModTime
                + ", winningByteSize=" + winningByteSize
                + ", winningSource=" + winningSource
                + ", actions=" + actions
                + ", classifications=" + classifications
                + "]";
    }
}
