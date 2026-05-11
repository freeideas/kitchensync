package gitignore.pattern.compiler;

public record Diagnostic(int lineNumber, String lineText, String reason) {}
