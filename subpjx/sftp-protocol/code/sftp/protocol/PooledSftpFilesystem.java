package sftp.protocol;

import bounded.resource.pool.ResourceLease;

public final class PooledSftpFilesystem extends SftpFilesystem {
    private final ResourceLease<SftpSession> lease;
    private boolean closed;

    PooledSftpFilesystem(SftpLocation location, ResourceLease<SftpSession> lease) {
        super(location, lease.resource(), () -> {
        });
        this.lease = lease;
    }

    @Override
    public void close() {
        if (closed) {
            return;
        }
        closed = true;
        if (!usable()) {
            lease.invalidate();
        }
        lease.close();
    }
}
