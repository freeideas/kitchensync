package sftp.protocol;

import org.apache.sshd.sftp.client.SftpClient;
import java.io.IOException;

public final class WriteHandle {
    private final SftpClient sftp;
    private final SftpClient.CloseableHandle handle;
    private long offset = 0;

    WriteHandle(SftpClient sftp, SftpClient.CloseableHandle handle) {
        this.sftp = sftp;
        this.handle = handle;
    }

    public void write(byte[] bytes) throws IOException {
        sftp.write(handle, offset, bytes, 0, bytes.length);
        offset += bytes.length;
    }

    public void close() throws IOException {
        sftp.close(handle);
    }
}
