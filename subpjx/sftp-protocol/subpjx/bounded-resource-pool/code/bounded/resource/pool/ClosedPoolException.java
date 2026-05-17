package bounded.resource.pool;

public final class ClosedPoolException extends IllegalStateException {
    public ClosedPoolException() {
        super("pool is closed");
    }
}
