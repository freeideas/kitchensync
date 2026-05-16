package sftp.protocol;

import net.schmizz.sshj.sftp.RemoteFile;

import java.io.IOException;

public final class WriteHandle implements AutoCloseable {
    private final RemoteFile file;
    private long offset;
    private boolean closed;

    WriteHandle(RemoteFile file) {
        this.file = file;
    }

    void write(byte[] bytes) throws SftpException {
        if (closed) {
            throw new SftpException(SftpError.io_error, "write handle is closed");
        }
        try {
            file.write(offset, bytes, 0, bytes.length);
            offset += bytes.length;
        } catch (IOException e) {
            throw SftpSession.map(e);
        }
    }

    @Override
    public void close() throws SftpException {
        if (!closed) {
            closed = true;
            try {
                file.close();
            } catch (IOException e) {
                throw SftpSession.map(e);
            }
        }
    }
}
