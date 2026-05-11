package gitignore.pattern.compiler;

import java.util.List;

public final class PatternSet {
    private final List<CompiledPattern> patterns;

    public PatternSet(List<CompiledPattern> patterns) {
        this.patterns = List.copyOf(patterns);
    }

    public int size() { return patterns.size(); }

    public CompiledPattern at(int i) { return patterns.get(i); }

    public List<CompiledPattern> patterns() { return patterns; }

    public static PatternSet empty() {
        return new PatternSet(List.of());
    }
}
