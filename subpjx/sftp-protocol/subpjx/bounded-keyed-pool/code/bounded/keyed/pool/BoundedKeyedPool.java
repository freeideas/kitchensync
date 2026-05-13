package bounded.keyed.pool;

import java.util.*;
import java.util.concurrent.*;
import java.util.function.*;

public final class BoundedKeyedPool<K, R> {
    private final Function<K, R> factory;
    private final Consumer<R> destructor;
    private final int maxPerKey;
    private final long idleTtlMillis;
    private final ScheduledExecutorService scheduler;

    private final Object lock = new Object();
    private volatile boolean shutdown = false;
    private final Map<K, PerKeyState> perKey = new HashMap<>();

    private final class PerKeyState {
        int reservedCount = 0;
        final Set<R> heldSet = new LinkedHashSet<>();
        final Deque<R> idleQueue = new ArrayDeque<>();
        final Map<R, ScheduledFuture<?>> idleTimers = new HashMap<>();
        final Deque<CompletableFuture<R>> waiters = new ArrayDeque<>();

        int liveCount() {
            return reservedCount + heldSet.size() + idleQueue.size();
        }
    }

    public BoundedKeyedPool(Function<K, R> factory, Consumer<R> destructor,
                             int maxPerKey, double idleTtlSeconds) {
        this.factory = factory;
        this.destructor = destructor;
        this.maxPerKey = maxPerKey;
        this.idleTtlMillis = (long) (idleTtlSeconds * 1000);
        this.scheduler = Executors.newSingleThreadScheduledExecutor(r -> {
            Thread t = new Thread(r, "pool-idle-timer");
            t.setDaemon(true);
            return t;
        });
    }

    public Handle<K, R> acquire(K key) throws InterruptedException {
        CompletableFuture<R> waiterFuture = null;
        boolean shouldCreate = false;

        synchronized (lock) {
            if (shutdown) throw new PoolShutdownException();
            PerKeyState state = perKey.computeIfAbsent(key, k -> new PerKeyState());

            if (!state.idleQueue.isEmpty()) {
                R resource = state.idleQueue.pollFirst();
                ScheduledFuture<?> timer = state.idleTimers.remove(resource);
                if (timer != null) timer.cancel(false);
                state.heldSet.add(resource);
                return new Handle<>(key, resource);
            }

            if (state.liveCount() < maxPerKey) {
                state.reservedCount++;
                shouldCreate = true;
            } else {
                waiterFuture = new CompletableFuture<>();
                state.waiters.addLast(waiterFuture);
            }
        }

        if (shouldCreate) {
            return doCreate(key);
        }

        try {
            R resource = waiterFuture.get();
            if (resource != null) {
                // Idle resource transferred directly by the releaser (already in heldSet)
                return new Handle<>(key, resource);
            }
            // Slot freed by discard/TTL-expiry; reservedCount already incremented for us
            return doCreate(key);
        } catch (CancellationException e) {
            throw new PoolShutdownException();
        } catch (ExecutionException e) {
            Throwable cause = e.getCause();
            if (cause instanceof RuntimeException re) throw re;
            throw new RuntimeException(cause);
        }
    }

    // Called outside the lock. reservedCount has been incremented by the caller.
    private Handle<K, R> doCreate(K key) {
        R resource;
        try {
            resource = factory.apply(key);
        } catch (Throwable t) {
            synchronized (lock) {
                PerKeyState state = perKey.get(key);
                if (state != null) {
                    state.reservedCount--;
                    signalWaiterForSlot(state);
                }
            }
            if (t instanceof RuntimeException re) throw re;
            throw new RuntimeException(t);
        }

        synchronized (lock) {
            PerKeyState state = perKey.get(key);
            if (state != null) state.reservedCount--;
            if (shutdown) {
                destructor.accept(resource);
                throw new PoolShutdownException();
            }
            if (state != null) state.heldSet.add(resource);
        }
        return new Handle<>(key, resource);
    }

    // Must be called under lock. Gives the first waiter a reserved slot to create a resource.
    private void signalWaiterForSlot(PerKeyState state) {
        if (!state.waiters.isEmpty()) {
            CompletableFuture<R> waiter = state.waiters.pollFirst();
            state.reservedCount++;
            waiter.complete(null); // signal: please create
        }
    }

    public void release(Handle<K, R> handle) {
        K key = handle.key();
        R resource = handle.resource();
        R toDestroy = null;

        synchronized (lock) {
            PerKeyState state = perKey.get(key);
            if (state == null) return;
            state.heldSet.remove(resource);

            if (!state.waiters.isEmpty()) {
                CompletableFuture<R> waiter = state.waiters.pollFirst();
                state.heldSet.add(resource); // transfer ownership to waiter
                waiter.complete(resource);
                return;
            }

            if (shutdown) {
                toDestroy = resource;
            } else if (idleTtlMillis <= 0) {
                toDestroy = resource; // TTL=0: expire immediately
            } else {
                state.idleQueue.addLast(resource);
                ScheduledFuture<?> timer = scheduler.schedule(
                    () -> expireIdle(key, resource),
                    idleTtlMillis, TimeUnit.MILLISECONDS);
                state.idleTimers.put(resource, timer);
            }
        }

        if (toDestroy != null) {
            destructor.accept(toDestroy);
        }
    }

    private void expireIdle(K key, R resource) {
        boolean shouldDestroy = false;
        synchronized (lock) {
            PerKeyState state = perKey.get(key);
            if (state == null) return;
            if (state.idleQueue.remove(resource)) {
                state.idleTimers.remove(resource);
                shouldDestroy = true;
                signalWaiterForSlot(state);
            }
        }
        if (shouldDestroy) {
            destructor.accept(resource);
        }
    }

    public void discard(Handle<K, R> handle) {
        K key = handle.key();
        R resource = handle.resource();

        synchronized (lock) {
            PerKeyState state = perKey.get(key);
            if (state != null) {
                state.heldSet.remove(resource);
                signalWaiterForSlot(state);
            }
        }
        destructor.accept(resource);
    }

    public void shutdown() {
        List<R> toDestroy = new ArrayList<>();
        List<CompletableFuture<R>> toCancel = new ArrayList<>();

        synchronized (lock) {
            shutdown = true;
            for (PerKeyState state : perKey.values()) {
                for (ScheduledFuture<?> timer : state.idleTimers.values()) {
                    timer.cancel(false);
                }
                toDestroy.addAll(state.idleQueue);
                toDestroy.addAll(state.heldSet);
                toCancel.addAll(state.waiters);
                state.idleTimers.clear();
                state.idleQueue.clear();
                state.heldSet.clear();
                state.waiters.clear();
            }
        }

        for (CompletableFuture<R> f : toCancel) {
            f.cancel(true);
        }
        for (R r : toDestroy) {
            destructor.accept(r);
        }

        scheduler.shutdown();
    }
}
