package bounded.resource.pool;

@FunctionalInterface
public interface PoolListener<K> {
    void on_event(PoolEvent<K> event);
}
