package sftp.protocol;

import bounded.resource.pool.BoundedPool;
import bounded.resource.pool.ClosedPoolException;
import bounded.resource.pool.ResourceLease;

public final class SftpTransferPool implements AutoCloseable {
    private final SftpLocation location;
    private final BoundedPool<SftpSession> pool;

    SftpTransferPool(SftpLocation location, BoundedPool<SftpSession> pool) {
        this.location = location;
        this.pool = pool;
    }

    public PooledSftpFilesystem acquire() throws SftpException {
        try {
            ResourceLease<SftpSession> lease = pool.acquire();
            return new PooledSftpFilesystem(location, lease);
        } catch (SftpException e) {
            throw e;
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new SftpException(SftpError.io_error, "acquire interrupted", e);
        } catch (ClosedPoolException e) {
            throw new SftpException(SftpError.io_error, "pool is closed", e);
        } catch (Exception e) {
            throw new SftpException(SftpError.io_error, e.getMessage() == null ? "sftp operation failed" : e.getMessage(), e);
        }
    }

    @Override
    public void close() {
        // Pool lifetime is owned by SftpPoolRegistry.
    }
}
