package url.normalizer;

public class UrlNormalizerError extends Exception {

    private final String code;

    public UrlNormalizerError(Code code) {
        super(code.code);
        this.code = code.code;
    }

    public String code() {
        return code;
    }

    public enum Code {
        INVALID_URL("invalid_url"),
        UNSUPPORTED_SCHEME("unsupported_scheme"),
        INVALID_PORT("invalid_port"),
        INVALID_PERCENT_ENCODING("invalid_percent_encoding");

        private final String code;

        Code(String code) {
            this.code = code;
        }
    }
}

