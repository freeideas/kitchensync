package staged.file.transfer;

public final class TransferException extends RuntimeException {
    private final TransferError error;

    public TransferException(TransferError error, String message) {
        super(message);
        this.error = error;
    }

    public TransferException(TransferError error, String message, Throwable cause) {
        super(message, cause);
        this.error = error;
    }

    public TransferError error() {
        return error;
    }
}
