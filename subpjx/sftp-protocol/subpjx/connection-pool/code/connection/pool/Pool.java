package connection.pool;

import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Deque;
import java.util.List;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.ScheduledFuture;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import java.util.function.Consumer;
import java.util.function.Supplier;

public final class Pool<C> {

    public interface EventListener {
        void onEvent(String kind, Object key, int inUse, int mc);
    }

    private static final ScheduledExecutorService TIMER =
            Executors.newScheduledThreadPool(2, r -> {
                Thread t = new Thread(r, "pool-timer");
                t.setDaemon(true);
                return t;
            });

    private static final ExecutorService OPENER =
            Executors.newCachedThreadPool(r -> {
                Thread t = new Thread(r, "pool-opener");
                t.setDaemon(true);
                return t;
            });

    private final Object key;
    private final Supplier<? extends C> opener;
    private final Consumer<? super C> closer;
    private final int mc;
    private final long ctMs;
    private final long kaMs;
    private final EventListener onEvent;

    private final Object lock = new Object();
    private int inUse = 0;
    private final Deque<Idle<C>> idle = new ArrayDeque<>();
    private boolean shutdown = false;

    private static final class Idle<C> {
        final C connection;
        ScheduledFuture<?> timer;
        Idle(C c) { this.connection = c; }
    }

    Pool(Object key,
         Supplier<? extends C> opener,
         Consumer<? super C> closer,
         PoolSettings settings,
         EventListener onEvent) {
        this.key = key;
        this.opener = opener;
        this.closer = closer;
        this.mc = settings.mc();
        this.ctMs = (long) settings.ct() * 1000L;
        this.kaMs = (long) settings.ka() * 1000L;
        this.onEvent = onEvent;
    }

    public C acquire() {
        int snapshot;
        synchronized (lock) {
            while (true) {
                if (shutdown) {
                    throw new IllegalStateException("pool is shut down");
                }
                if (!idle.isEmpty()) {
                    Idle<C> entry = idle.removeFirst();
                    if (entry.timer != null) {
                        entry.timer.cancel(false);
                    }
                    inUse++;
                    snapshot = inUse;
                    fireEvent("acquire", snapshot);
                    return entry.connection;
                }
                if (inUse < mc) {
                    inUse++;
                    snapshot = inUse;
                    break;
                }
                try {
                    lock.wait();
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    throw new RuntimeException(e);
                }
            }
        }
        C conn;
        try {
            conn = openWithTimeout();
        } catch (RuntimeException t) {
            synchronized (lock) {
                inUse--;
                lock.notifyAll();
            }
            throw t;
        }
        fireEvent("acquire", snapshot);
        return conn;
    }

    public void release(C conn) {
        boolean wasShutdown;
        int snapshot;
        synchronized (lock) {
            inUse--;
            snapshot = inUse;
            wasShutdown = shutdown;
            if (!shutdown) {
                Idle<C> entry = new Idle<>(conn);
                entry.timer = TIMER.schedule(() -> expireIdle(entry), kaMs, TimeUnit.MILLISECONDS);
                idle.addLast(entry);
            }
            lock.notifyAll();
        }
        if (wasShutdown) {
            safeClose(conn);
        }
        fireEvent("release", snapshot);
    }

    public void closePool() {
        List<C> toClose = new ArrayList<>();
        synchronized (lock) {
            shutdown = true;
            for (Idle<C> entry : idle) {
                if (entry.timer != null) {
                    entry.timer.cancel(false);
                }
                toClose.add(entry.connection);
            }
            idle.clear();
            lock.notifyAll();
        }
        for (C c : toClose) {
            safeClose(c);
        }
    }

    private void expireIdle(Idle<C> entry) {
        boolean removed;
        synchronized (lock) {
            removed = idle.remove(entry);
            if (removed) {
                lock.notifyAll();
            }
        }
        if (removed) {
            safeClose(entry.connection);
        }
    }

    private C openWithTimeout() {
        if (ctMs <= 0) {
            return opener.get();
        }
        Future<C> future = OPENER.submit(opener::get);
        try {
            return future.get(ctMs, TimeUnit.MILLISECONDS);
        } catch (TimeoutException e) {
            future.cancel(true);
            throw new RuntimeException("open timed out after " + ctMs + "ms");
        } catch (ExecutionException e) {
            future.cancel(true);
            Throwable cause = e.getCause();
            if (cause instanceof RuntimeException r) throw r;
            if (cause instanceof Error err) throw err;
            throw new RuntimeException(cause);
        } catch (InterruptedException e) {
            future.cancel(true);
            Thread.currentThread().interrupt();
            throw new RuntimeException(e);
        }
    }

    private void safeClose(C c) {
        try {
            closer.accept(c);
        } catch (Throwable t) {
            // close callback never fails observably per spec
        }
    }

    private void fireEvent(String kind, int inUseSnapshot) {
        if (onEvent == null) return;
        try {
            onEvent.onEvent(kind, key, inUseSnapshot, mc);
        } catch (Throwable t) {
            // observation never affects pool state
        }
    }

    public int mc() { return mc; }

    public Object key() { return key; }
}
