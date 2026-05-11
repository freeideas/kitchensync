package ssh.sftp.session;

import java.util.UUID;

/** Buffered remote file content + read offset. The read happens once at open-time
 *  (the spec only requires that successive reads yield the file's full content). */
public final class ReadHandle {
    public final String id = "rh-" + UUID.randomUUID();
    public final Session session;
    public final byte[] data;
    public int offset;

    public ReadHandle(Session session, byte[] data) {
        this.session = session;
        this.data = data;
        this.offset = 0;
    }
}
