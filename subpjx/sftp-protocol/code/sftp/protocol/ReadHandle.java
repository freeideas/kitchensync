package sftp.protocol;

import net.schmizz.sshj.sftp.RemoteFile;

import java.io.IOException;
import java.util.Arrays;

public final class ReadHandle implements AutoCloseable {
    private final RemoteFile file;
    private long offset;
    private boolean closed;

    ReadHandle(RemoteFile file) {
        this.file = file;
    }

    byte[] read(int maxBytes) throws SftpException {
        if (closed) {
            throw new SftpException(SftpError.io_error, "read handle is closed");
        }
        if (maxBytes <= 0) {
            throw new IllegalArgumentException("max_bytes must be positive");
        }
        byte[] buffer = new byte[maxBytes];
        try {
            int count = file.read(offset, buffer, 0, maxBytes);
            if (count < 0) {
                return null;
            }
            offset += count;
            return count == buffer.length ? buffer : Arrays.copyOf(buffer, count);
        } catch (IOException e) {
            throw SftpSession.map(e);
        }
    }

    @Override
    public void close() {
        if (!closed) {
            closed = true;
            try {
                file.close();
            } catch (IOException ignored) {
            }
        }
    }
}
