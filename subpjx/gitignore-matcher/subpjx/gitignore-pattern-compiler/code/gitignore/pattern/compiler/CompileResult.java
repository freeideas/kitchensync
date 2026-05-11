package gitignore.pattern.compiler;

import java.util.List;

public record CompileResult(PatternSet patternSet, List<Diagnostic> diagnostics) {}
