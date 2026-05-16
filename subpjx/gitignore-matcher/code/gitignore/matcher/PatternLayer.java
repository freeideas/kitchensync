package gitignore.matcher;

public record PatternLayer(String basePath, String patternText, String sourceName) {
    public PatternLayer {
        basePath = basePath == null ? "" : basePath;
        patternText = patternText == null ? "" : patternText;
    }

    public PatternLayer(String basePath, String patternText) {
        this(basePath, patternText, null);
    }
}
