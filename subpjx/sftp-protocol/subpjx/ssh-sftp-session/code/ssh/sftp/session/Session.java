package ssh.sftp.session;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;

/** One open SSH+SFTP session, backed by an OpenSSH ControlMaster connection.
 *  Operations dispatch ssh subprocesses that reuse the master socket. */
public final class Session {

    public final String id = "ssh-sftp-" + UUID.randomUUID();
    public final String host;
    public final int port;
    public final String user;
    public final String controlSocket;

    /** Open read handles indexed by handle ID. */
    public final ConcurrentHashMap<String, ReadHandle> readHandles = new ConcurrentHashMap<>();
    public final ConcurrentHashMap<String, WriteHandle> writeHandles = new ConcurrentHashMap<>();

    private volatile boolean closed = false;

    Session(String host, int port, String user, String controlSocket) {
        this.host = host;
        this.port = port;
        this.user = user;
        this.controlSocket = controlSocket;
    }

    public boolean isClosed() { return closed; }

    /** Build the ssh argv to run a single remote shell command via the control socket. */
    public List<String> sshArgv(String remoteCmd) {
        List<String> a = new ArrayList<>();
        a.add("ssh");
        a.add("-S"); a.add(controlSocket);
        a.add("-p"); a.add(String.valueOf(port));
        a.add("-o"); a.add("StrictHostKeyChecking=yes");
        a.add("-o"); a.add("BatchMode=yes");
        a.add(user + "@" + host);
        a.add(remoteCmd);
        return a;
    }

    /** Tear down the master and remove the socket. Idempotent. */
    public synchronized void close() {
        if (closed) return;
        closed = true;
        try {
            ProcessBuilder pb = new ProcessBuilder(
                    "ssh", "-S", controlSocket,
                    "-o", "BatchMode=yes",
                    "-O", "exit",
                    user + "@" + host
            ).redirectErrorStream(true);
            Process p = pb.start();
            p.waitFor();
        } catch (Exception ignored) {
        }
        try { Files.deleteIfExists(Path.of(controlSocket)); } catch (Exception ignored) {}
    }
}
