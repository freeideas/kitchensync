package gitignore.pattern.syntax;

public record PatternRule(
        String pattern,
        boolean negated,
        boolean directoryOnly,
        boolean anchored,
        boolean hasSlash,
        String regex) {
}
