package sftp.protocol;

import org.apache.sshd.sftp.client.SftpClient;
import java.io.IOException;
import java.util.Arrays;

public final class ReadHandle {
    private final SftpClient sftp;
    private final SftpClient.CloseableHandle handle;
    private long offset = 0;
    private boolean eof = false;

    ReadHandle(SftpClient sftp, SftpClient.CloseableHandle handle) {
        this.sftp = sftp;
        this.handle = handle;
    }

    // Returns null on EOF, otherwise returns bytes read (length <= maxBytes)
    public byte[] read(int maxBytes) throws IOException {
        if (eof) return null;
        byte[] buf = new byte[maxBytes];
        int n = sftp.read(handle, offset, buf, 0, maxBytes);
        if (n <= 0) {
            eof = true;
            return null;
        }
        offset += n;
        return n == maxBytes ? buf : Arrays.copyOf(buf, n);
    }

    public void close() throws IOException {
        sftp.close(handle);
    }
}
