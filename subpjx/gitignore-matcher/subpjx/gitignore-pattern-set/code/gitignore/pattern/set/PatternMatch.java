package gitignore.pattern.set;

public record PatternMatch(
        PatternDecision decision,
        boolean negated,
        String source_name,
        Integer line_number,
        String pattern
) {
    public static PatternMatch none() {
        return new PatternMatch(PatternDecision.none, false, null, null, null);
    }
}
