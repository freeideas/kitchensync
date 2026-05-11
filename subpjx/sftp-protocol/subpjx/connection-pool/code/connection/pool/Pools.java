package connection.pool;

import java.util.HashMap;
import java.util.Map;
import java.util.function.Consumer;
import java.util.function.Supplier;

public final class Pools {

    private final Map<Object, Pool<?>> registry = new HashMap<>();

    @SuppressWarnings("unchecked")
    public synchronized <C> Pool<C> register(
            Object key,
            Supplier<? extends C> open,
            Consumer<? super C> close,
            PoolSettings settings,
            Pool.EventListener onEvent) {
        Pool<C> existing = (Pool<C>) registry.get(key);
        if (existing != null) {
            return existing;
        }
        Pool<C> created = new Pool<>(key, open, close, settings, onEvent);
        registry.put(key, created);
        return created;
    }

    public synchronized void closePool(Pool<?> pool) {
        pool.closePool();
        registry.values().removeIf(p -> p == pool);
    }
}
