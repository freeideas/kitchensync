package gitignore.matcher;

import gitignore.pattern.set.GitignorePatternSet;
import gitignore.pattern.set.GitignorePatternSetException;
import gitignore.pattern.set.PatternDecision;
import gitignore.pattern.set.PatternMatch;
import gitignore.pattern.set.PatternSetSource;

import java.util.ArrayList;
import java.util.List;
import java.util.Objects;
import java.util.Set;

public final class IgnoreMatcher {
    private final List<LayerRule> rules;
    private final IgnoreOptions options;

    private IgnoreMatcher(List<LayerRule> rules, IgnoreOptions options) {
        this.rules = List.copyOf(rules);
        this.options = options;
    }

    public static IgnoreMatcher compile(List<PatternLayer> layers, IgnoreOptions options) {
        Objects.requireNonNull(layers, "layers");
        IgnoreOptions resolvedOptions = options == null ? IgnoreOptions.defaults() : options;
        ArrayList<LayerRule> parsedRules = new ArrayList<>();
        for (PatternLayer layer : layers) {
            parsedRules.addAll(compileLayer(Objects.requireNonNull(layer, "layer")));
        }
        return new IgnoreMatcher(parsedRules, resolvedOptions);
    }

    public static IgnoreMatcher empty(IgnoreOptions options) {
        return new IgnoreMatcher(List.of(), options == null ? IgnoreOptions.defaults() : options);
    }

    public IgnoreMatcher extend(PatternLayer layer) {
        ArrayList<LayerRule> extended = new ArrayList<>(rules);
        extended.addAll(compileLayer(Objects.requireNonNull(layer, "layer")));
        return new IgnoreMatcher(extended, options);
    }

    public MatchResult match(PathEntry entry) {
        Objects.requireNonNull(entry, "entry");
        PathValidator.validateEntryPath(entry.relativePath());

        MatchResult nonOverridable = nonOverridableBuiltIn(entry);
        if (nonOverridable != null) {
            return nonOverridable;
        }

        ArrayList<MatchState> states = pathStates(entry);
        for (MatchState state : states) {
            if (state.directory() && options.defaultExcludedDirectoryNames().contains(basename(state.path()))) {
                state.apply(defaultBuiltIn());
            }
        }

        for (int order = 0; order < rules.size(); order++) {
            LayerRule rule = rules.get(order);
            for (int i = 0; i < states.size(); i++) {
                MatchState state = states.get(i);
                if (hasIgnoredAncestor(states, i)) {
                    continue;
                }
                MatchResult result = rule.match(state.path(), state.kind());
                if (result != null) {
                    state.apply(result);
                }
            }
        }

        MatchState entryState = states.get(states.size() - 1);
        MatchState ignoredAncestor = null;
        MatchState includedAncestor = null;
        for (int i = 0; i < states.size() - 1; i++) {
            MatchState state = states.get(i);
            if (state.ignored()) {
                ignoredAncestor = state;
            } else if (state.result() != null) {
                includedAncestor = state;
            }
        }
        if (ignoredAncestor != null) {
            return ignoredAncestor.result();
        }
        if (entryState.result() != null) {
            return entryState.result();
        }
        return includedAncestor == null ? MatchResult.none() : includedAncestor.result();
    }

    public List<PathEntry> filter(List<PathEntry> entries) {
        Objects.requireNonNull(entries, "entries");
        ArrayList<PathEntry> kept = new ArrayList<>();
        for (PathEntry entry : entries) {
            if (!match(entry).ignored()) {
                kept.add(entry);
            }
        }
        return List.copyOf(kept);
    }

    private MatchResult nonOverridableBuiltIn(PathEntry entry) {
        if (options.ignoreSymlinks() && entry.kind() == EntryKind.symlink) {
            return new MatchResult(true, RuleKind.always_builtin, false, null, null, null);
        }
        if (options.ignoreSpecialEntries() && entry.kind() == EntryKind.special) {
            return new MatchResult(true, RuleKind.always_builtin, false, null, null, null);
        }
        if (directoryNameApplies(entry, options.alwaysExcludedDirectoryNames())) {
            return new MatchResult(true, RuleKind.always_builtin, false, null, null, null);
        }
        return null;
    }

    private static MatchResult defaultBuiltIn() {
        return new MatchResult(true, RuleKind.default_builtin, false, null, null, null);
    }

    private static List<LayerRule> compileLayer(PatternLayer layer) {
        PathValidator.validateBasePath(layer.basePath());
        String[] lines = layer.patternText().split("\n", -1);
        ArrayList<LayerRule> layerRules = new ArrayList<>();
        for (int i = 0; i < lines.length; i++) {
            String line = stripCarriageReturn(lines[i]);
            try {
                layerRules.add(new LayerRule(
                        layer.basePath(),
                        i + 1,
                        line,
                        GitignorePatternSet.compile(new PatternSetSource(line, layer.sourceName()))));
            } catch (GitignorePatternSetException ex) {
                throw new IgnoreMatcherException(ex.category().name(), ex.getMessage());
            }
        }
        return layerRules;
    }

    private static String stripCarriageReturn(String line) {
        return line.endsWith("\r") ? line.substring(0, line.length() - 1) : line;
    }

    private static ArrayList<MatchState> pathStates(PathEntry entry) {
        ArrayList<MatchState> states = new ArrayList<>();
        String[] segments = entry.relativePath().split("/");
        StringBuilder path = new StringBuilder();
        for (int i = 0; i < segments.length; i++) {
            if (i > 0) {
                path.append('/');
            }
            path.append(segments[i]);
            EntryKind kind = i < segments.length - 1 ? EntryKind.directory : entry.kind();
            states.add(new MatchState(path.toString(), kind));
        }
        return states;
    }

    private static boolean hasIgnoredAncestor(List<MatchState> states, int index) {
        for (int i = 0; i < index; i++) {
            if (states.get(i).ignored()) {
                return true;
            }
        }
        return false;
    }

    private static String basename(String path) {
        int slash = path.lastIndexOf('/');
        return slash < 0 ? path : path.substring(slash + 1);
    }

    private static boolean directoryNameApplies(PathEntry entry, Set<String> names) {
        String[] segments = entry.relativePath().split("/");
        for (int i = 0; i < segments.length; i++) {
            boolean segmentIsDirectory = i < segments.length - 1 || entry.kind() == EntryKind.directory;
            if (segmentIsDirectory && names.contains(segments[i])) {
                return true;
            }
        }
        return false;
    }

    private static final class MatchState {
        private final String path;
        private final EntryKind kind;
        private MatchResult result;

        MatchState(String path, EntryKind kind) {
            this.path = path;
            this.kind = kind;
        }

        String path() {
            return path;
        }

        boolean directory() {
            return kind == EntryKind.directory;
        }

        EntryKind kind() {
            return kind;
        }

        boolean ignored() {
            return result != null && result.ignored();
        }

        MatchResult result() {
            return result;
        }

        void apply(MatchResult nextResult) {
            result = nextResult;
        }
    }

    private static final class LayerRule {
        private final String basePath;
        private final int lineNumber;
        private final String pattern;
        private final GitignorePatternSet patternSet;

        LayerRule(String basePath, int lineNumber, String pattern, GitignorePatternSet patternSet) {
            this.basePath = basePath;
            this.lineNumber = lineNumber;
            this.pattern = pattern;
            this.patternSet = patternSet;
        }

        MatchResult match(String path, EntryKind kind) {
            String relative = relativeToBase(path);
            if (relative == null || relative.isEmpty()) {
                return null;
            }
            PatternMatch match;
            try {
                match = patternSet.match(new gitignore.pattern.set.PathEntry(relative, childKind(kind)));
            } catch (GitignorePatternSetException ex) {
                throw new IgnoreMatcherException(ex.category().name(), ex.getMessage());
            }
            if (match.decision() == PatternDecision.none) {
                return null;
            }
            return new MatchResult(
                    match.decision() == PatternDecision.ignore,
                    RuleKind.pattern,
                    match.negated(),
                    match.source_name(),
                    lineNumber,
                    pattern);
        }

        private String relativeToBase(String path) {
            if (basePath.isEmpty()) {
                return path;
            }
            if (!path.startsWith(basePath + "/")) {
                return null;
            }
            return path.substring(basePath.length() + 1);
        }

        private static gitignore.pattern.set.EntryKind childKind(EntryKind kind) {
            return gitignore.pattern.set.EntryKind.valueOf(kind.name());
        }
    }
}
