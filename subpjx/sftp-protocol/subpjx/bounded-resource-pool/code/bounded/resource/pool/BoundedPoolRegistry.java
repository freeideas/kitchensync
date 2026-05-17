package bounded.resource.pool;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.ThreadFactory;
import java.util.concurrent.TimeUnit;

public final class BoundedPoolRegistry<K, R> implements AutoCloseable {
    private final Map<K, KeyedBoundedPool<K, R>> pools = new HashMap<>();
    private final ScheduledExecutorService expiryExecutor;
    private boolean closed;

    public BoundedPoolRegistry() {
        ThreadFactory factory = runnable -> {
            Thread thread = new Thread(runnable, "bounded-resource-pool-expiry");
            thread.setDaemon(true);
            return thread;
        };
        expiryExecutor = Executors.newSingleThreadScheduledExecutor(factory);
    }

    public synchronized BoundedPool<R> pool_for(
            K key,
            PoolSettings settings,
            ResourceFactory<K, R> factory,
            PoolListener<K> listener) {
        Objects.requireNonNull(settings, "settings");
        Objects.requireNonNull(factory, "factory");
        return pools.computeIfAbsent(
                key,
                ignored -> new KeyedBoundedPool<>(this, key, settings, factory, listener, expiryExecutor));
    }

    synchronized boolean is_closed() {
        return closed;
    }

    @Override
    public void close() {
        List<KeyedBoundedPool<K, R>> snapshot;
        synchronized (this) {
            if (closed) {
                return;
            }
            closed = true;
            snapshot = new ArrayList<>(pools.values());
        }
        for (KeyedBoundedPool<K, R> pool : snapshot) {
            pool.close_owned_resources();
        }
        expiryExecutor.shutdownNow();
    }
}
