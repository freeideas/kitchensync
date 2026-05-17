package bounded.resource.pool;

public interface ResourceLease<R> extends AutoCloseable {
    R resource();

    void invalidate();

    @Override
    void close();
}
