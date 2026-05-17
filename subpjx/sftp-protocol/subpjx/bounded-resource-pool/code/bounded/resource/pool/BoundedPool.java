package bounded.resource.pool;

public interface BoundedPool<R> {
    ResourceLease<R> acquire() throws Exception;
}
