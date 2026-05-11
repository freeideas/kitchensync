package ssh.sftp.session;

import java.io.ByteArrayOutputStream;
import java.util.UUID;

/** Accumulates bytes from chunked writes; close-write flushes them to the remote path. */
public final class WriteHandle {
    public final String id = "wh-" + UUID.randomUUID();
    public final Session session;
    public final String path;
    public final ByteArrayOutputStream buffer = new ByteArrayOutputStream();

    public WriteHandle(Session session, String path) {
        this.session = session;
        this.path = path;
    }
}
