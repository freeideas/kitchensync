package bounded.resource.pool;

import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.ScheduledFuture;
import java.util.concurrent.TimeUnit;

final class KeyedBoundedPool<K, R> implements BoundedPool<R> {
    private final BoundedPoolRegistry<K, R> registry;
    private final K key;
    private final PoolSettings settings;
    private final ResourceFactory<K, R> factory;
    private final PoolListener<K> listener;
    private final ScheduledExecutorService expiryExecutor;
    private final ArrayDeque<Entry<R>> idle = new ArrayDeque<>();
    private final List<Entry<R>> entries = new ArrayList<>();
    private boolean closed;
    private int openResources;

    KeyedBoundedPool(
            BoundedPoolRegistry<K, R> registry,
            K key,
            PoolSettings settings,
            ResourceFactory<K, R> factory,
            PoolListener<K> listener,
            ScheduledExecutorService expiryExecutor) {
        this.registry = registry;
        this.key = key;
        this.settings = settings;
        this.factory = factory;
        this.listener = listener;
        this.expiryExecutor = expiryExecutor;
    }

    @Override
    public ResourceLease<R> acquire() throws Exception {
        Entry<R> entry;
        synchronized (this) {
            while (true) {
                ensureOpen();
                entry = idle.pollFirst();
                if (entry != null) {
                    entry.cancelExpiry();
                    entry.leased = true;
                    fireEvent();
                    return new Lease(entry);
                }
                if (openResources < settings.max_resources()) {
                    openResources++;
                    break;
                }
                try {
                    wait();
                } catch (InterruptedException interrupted) {
                    Thread.currentThread().interrupt();
                    throw interrupted;
                }
            }
        }

        R resource;
        try {
            resource = factory.open(key);
        } catch (Exception failure) {
            synchronized (this) {
                openResources--;
                notifyAll();
            }
            throw failure;
        }

        synchronized (this) {
            if (closed || registry.is_closed()) {
                openResources--;
                closeIgnoringFailure(resource);
                notifyAll();
                throw new ClosedPoolException();
            }
            entry = new Entry<>(resource);
            entry.leased = true;
            entries.add(entry);
            fireEvent();
            return new Lease(entry);
        }
    }

    void close_owned_resources() {
        List<Entry<R>> toClose = new ArrayList<>();
        synchronized (this) {
            if (closed) {
                return;
            }
            closed = true;
            for (Entry<R> entry : entries) {
                if (!entry.closed) {
                    entry.closed = true;
                    entry.cancelExpiry();
                    toClose.add(entry);
                }
            }
            idle.clear();
            openResources = 0;
            notifyAll();
        }
        for (Entry<R> entry : toClose) {
            closeIgnoringFailure(entry.resource);
        }
    }

    private void release(Entry<R> entry, boolean invalidated) {
        boolean closeResource = false;
        synchronized (this) {
            if (entry.leaseClosed) {
                return;
            }
            entry.leaseClosed = true;
            entry.leased = false;
            if (entry.closed) {
                fireEvent();
                return;
            }
            if (invalidated || closed || registry.is_closed()) {
                entry.closed = true;
                openResources--;
                closeResource = true;
                notifyAll();
            } else {
                idle.addLast(entry);
                scheduleExpiry(entry);
                notifyAll();
            }
            fireEvent();
        }
        if (closeResource) {
            closeIgnoringFailure(entry.resource);
        }
    }

    private void expire(Entry<R> entry) {
        boolean closeResource = false;
        synchronized (this) {
            if (entry.closed || entry.leased || !idle.remove(entry)) {
                return;
            }
            entry.closed = true;
            openResources--;
            closeResource = true;
            notifyAll();
            fireEvent();
        }
        if (closeResource) {
            closeIgnoringFailure(entry.resource);
        }
    }

    private void scheduleExpiry(Entry<R> entry) {
        entry.cancelExpiry();
        entry.expiry = expiryExecutor.schedule(
                () -> expire(entry),
                settings.idle_keep_alive_ttl().toNanos(),
                TimeUnit.NANOSECONDS);
    }

    private void ensureOpen() {
        if (closed || registry.is_closed()) {
            throw new ClosedPoolException();
        }
    }

    private void fireEvent() {
        if (listener == null) {
            return;
        }
        try {
            listener.on_event(new PoolEvent<>(key, openResources, settings.max_resources()));
        } catch (RuntimeException ignored) {
        }
    }

    private void closeIgnoringFailure(R resource) {
        try {
            factory.close(resource);
        } catch (Exception ignored) {
        }
    }

    private static final class Entry<R> {
        private final R resource;
        private ScheduledFuture<?> expiry;
        private boolean leased;
        private boolean closed;
        private boolean leaseClosed;

        private Entry(R resource) {
            this.resource = resource;
        }

        private void cancelExpiry() {
            if (expiry != null) {
                expiry.cancel(false);
                expiry = null;
            }
            leaseClosed = false;
        }
    }

    private final class Lease implements ResourceLease<R> {
        private final Entry<R> entry;
        private boolean invalidated;

        private Lease(Entry<R> entry) {
            this.entry = entry;
        }

        @Override
        public R resource() {
            return entry.resource;
        }

        @Override
        public void invalidate() {
            invalidated = true;
        }

        @Override
        public void close() {
            release(entry, invalidated);
        }
    }
}
