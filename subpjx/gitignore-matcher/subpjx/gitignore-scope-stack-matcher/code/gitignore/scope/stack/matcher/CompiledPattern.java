package gitignore.scope.stack.matcher;

public record CompiledPattern(
    String body,
    boolean isNegation,
    boolean isAnchored,
    boolean isDirOnly
) {}
