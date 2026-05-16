package sync.decision.engine;

public final class InvalidInputException extends RuntimeException {
    public InvalidInputException() {
        super("invalid_input");
    }
}
