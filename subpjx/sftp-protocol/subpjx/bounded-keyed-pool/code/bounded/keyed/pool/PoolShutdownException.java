package bounded.keyed.pool;

public final class PoolShutdownException extends RuntimeException {
    public PoolShutdownException() {
        super("pool is shut down");
    }
}
