package url.normalizer;

public record NormalizedUrl(String normalized_url) {
    @Override
    public String toString() {
        return normalized_url();
    }
}

