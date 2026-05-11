package gitignore.scope.stack.matcher;

import java.util.List;

public record Layer(String scopeDir, List<CompiledPattern> patternSet) {}
