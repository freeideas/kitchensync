package sftp.protocol;

import org.apache.sshd.client.session.ClientSession;
import org.apache.sshd.sftp.client.SftpClient;

import java.util.concurrent.TimeUnit;

final class SftpSession {
    private final ClientSession sshSession;
    private final SftpClient sftpClient;

    SftpSession(ClientSession sshSession, SftpClient sftpClient) {
        this.sshSession = sshSession;
        this.sftpClient = sftpClient;
    }

    SftpClient sftp() { return sftpClient; }

    void close() {
        try { sftpClient.close(); } catch (Exception ignored) {}
        try { sshSession.close(true).await(5, TimeUnit.SECONDS); } catch (Exception ignored) {}
    }
}
