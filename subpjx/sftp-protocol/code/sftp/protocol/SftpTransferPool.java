package sftp.protocol;

import java.util.ArrayDeque;
import java.util.HashSet;
import java.util.Optional;
import java.util.Set;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.ScheduledFuture;
import java.util.concurrent.TimeUnit;

public final class SftpTransferPool implements AutoCloseable {
    private final SftpLocation location;
    private final SftpSettings settings;
    private final AuthConfig auth;
    private final Optional<SftpPoolListener> listener;
    private final ScheduledExecutorService scheduler = Executors.newSingleThreadScheduledExecutor(r -> {
        Thread thread = new Thread(r, "sftp-pool-idle");
        thread.setDaemon(true);
        return thread;
    });
    private final ArrayDeque<PooledSftpFilesystem> idle = new ArrayDeque<>();
    private final Set<PooledSftpFilesystem> borrowed = new HashSet<>();
    private int openConnections;
    private boolean closed;

    SftpTransferPool(
            SftpLocation location,
            SftpSettings settings,
            AuthConfig auth,
            Optional<SftpPoolListener> listener) {
        this.location = location;
        this.settings = settings;
        this.auth = auth;
        this.listener = listener;
    }

    public PooledSftpFilesystem acquire() throws SftpException {
        synchronized (this) {
            if (closed) {
                throw new SftpException(SftpError.io_error, "pool is closed");
            }
            while (true) {
                PooledSftpFilesystem existing = idle.pollFirst();
                if (existing != null) {
                    existing.cancelIdleClose();
                    existing.borrow();
                    borrowed.add(existing);
                    emit();
                    return existing;
                }
                if (openConnections < settings.max_connections()) {
                    openConnections++;
                    break;
                }
                try {
                    wait();
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    throw new SftpException(SftpError.io_error, "acquire interrupted", e);
                }
                if (closed) {
                    throw new SftpException(SftpError.io_error, "pool is closed");
                }
            }
        }

        PooledSftpFilesystem created = null;
        try {
            created = newPooledFilesystem();
            synchronized (this) {
                created.borrow();
                borrowed.add(created);
                emit();
                return created;
            }
        } catch (SftpException e) {
            synchronized (this) {
                openConnections--;
                notifyAll();
            }
            if (created != null) {
                created.closeUnderlying();
            }
            throw e;
        }
    }

    void release(PooledSftpFilesystem filesystem) {
        synchronized (this) {
            if (!borrowed.remove(filesystem)) {
                return;
            }
            if (closed || !filesystem.usable()) {
                filesystem.closeUnderlying();
                openConnections--;
                notifyAll();
                emit();
                return;
            }
            filesystem.markIdle();
            idle.addLast(filesystem);
            scheduleIdleClose(filesystem);
            notifyAll();
            emit();
        }
    }

    @Override
    public synchronized void close() {
        if (closed) {
            return;
        }
        closed = true;
        for (PooledSftpFilesystem filesystem : idle) {
            filesystem.cancelIdleClose();
            filesystem.closeUnderlying();
        }
        idle.clear();
        for (PooledSftpFilesystem filesystem : borrowed) {
            filesystem.closeUnderlying();
        }
        borrowed.clear();
        openConnections = 0;
        scheduler.shutdownNow();
        notifyAll();
    }

    private PooledSftpFilesystem newPooledFilesystem() throws SftpException {
        return new PooledSftpFilesystem(location, SftpSession.open(location, settings, auth), this);
    }

    private void scheduleIdleClose(PooledSftpFilesystem filesystem) {
        long ttlMillis = settings.idle_keep_alive_ttl().toMillis();
        ScheduledFuture<?> future = scheduler.schedule(() -> idleTimeout(filesystem), ttlMillis, TimeUnit.MILLISECONDS);
        filesystem.setIdleFuture(future);
    }

    private void idleTimeout(PooledSftpFilesystem filesystem) {
        synchronized (this) {
            if (!idle.remove(filesystem)) {
                return;
            }
            filesystem.closeUnderlying();
            openConnections--;
            notifyAll();
            emit();
        }
    }

    private void emit() {
        listener.ifPresent(l -> l.on_event(new PoolEvent(
                location.endpointKey(),
                openConnections,
                settings.max_connections())));
    }
}
