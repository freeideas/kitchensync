package gitignore.matcher;

public record MatchResult(
        boolean ignored,
        RuleKind ruleKind,
        boolean negated,
        String sourceName,
        Integer lineNumber,
        String pattern) {
    public static MatchResult none() {
        return new MatchResult(false, RuleKind.none, false, null, null, null);
    }
}
