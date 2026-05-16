package gitignore.pattern.set;

public record PatternSetSource(String pattern_text, String source_name) {
    public PatternSetSource(String pattern_text) {
        this(pattern_text, null);
    }
}
