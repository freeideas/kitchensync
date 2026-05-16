package sftp.protocol;

import java.util.concurrent.ScheduledFuture;

public final class PooledSftpFilesystem extends SftpFilesystem {
    private final SftpTransferPool pool;
    private ScheduledFuture<?> idleFuture;
    private boolean borrowed;

    PooledSftpFilesystem(SftpLocation location, SftpSession session, SftpTransferPool pool) {
        super(location, session, () -> {
        });
        this.pool = pool;
    }

    @Override
    public void close() {
        if (borrowed) {
            borrowed = false;
            pool.release(this);
        }
    }

    void borrow() {
        borrowed = true;
    }

    void markIdle() {
        borrowed = false;
    }

    void setIdleFuture(ScheduledFuture<?> idleFuture) {
        this.idleFuture = idleFuture;
    }

    void cancelIdleClose() {
        if (idleFuture != null) {
            idleFuture.cancel(false);
            idleFuture = null;
        }
    }
}
