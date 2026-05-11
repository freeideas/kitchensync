package rfc8089.file.uri;

public class FileUriException extends RuntimeException {
    public final Integer offset;

    public FileUriException(String message, Integer offset) {
        super(message);
        this.offset = offset;
    }
}
