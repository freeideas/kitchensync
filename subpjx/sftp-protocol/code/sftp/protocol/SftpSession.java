package sftp.protocol;

import com.jcraft.jsch.agentproxy.AgentProxy;
import com.jcraft.jsch.agentproxy.AgentProxyException;
import com.jcraft.jsch.agentproxy.Connector;
import com.jcraft.jsch.agentproxy.ConnectorFactory;
import com.jcraft.jsch.agentproxy.Identity;
import com.jcraft.jsch.agentproxy.sshj.AuthAgent;
import net.schmizz.sshj.SSHClient;
import net.schmizz.sshj.common.Buffer;
import net.schmizz.sshj.sftp.Response;
import net.schmizz.sshj.sftp.SFTPClient;
import net.schmizz.sshj.sftp.SFTPException;
import net.schmizz.sshj.transport.TransportException;
import net.schmizz.sshj.userauth.UserAuthException;
import net.schmizz.sshj.userauth.method.AuthMethod;
import net.schmizz.sshj.userauth.method.AuthPassword;
import net.schmizz.sshj.userauth.method.AuthPublickey;
import net.schmizz.sshj.userauth.password.PasswordUtils;
import net.schmizz.sshj.userauth.keyprovider.KeyProvider;

import java.io.IOException;
import java.io.FileNotFoundException;
import java.io.RandomAccessFile;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

final class SftpSession implements AutoCloseable {
    private final SSHClient ssh;
    private final SFTPClient sftp;

    private SftpSession(SSHClient ssh, SFTPClient sftp) {
        this.ssh = ssh;
        this.sftp = sftp;
    }

    static SftpSession open(SftpLocation location, SftpSettings settings, AuthConfig auth) throws SftpException {
        silenceSlf4jNoProviderWarning();
        SftpException firstTimeout = null;
        for (int attempt = 0; attempt < 2; attempt++) {
            try {
                return openOnce(location, settings, auth);
            } catch (SftpException e) {
                if (attempt == 0 && isConnectTimeout(e)) {
                    firstTimeout = e;
                    continue;
                }
                throw e;
            }
        }
        throw firstTimeout;
    }

    private static SftpSession openOnce(SftpLocation location, SftpSettings settings, AuthConfig auth) throws SftpException {
        SSHClient ssh = new SSHClient();
        try {
            int timeoutMillis = timeoutMillis(settings);
            ssh.setConnectTimeout(timeoutMillis);
            ssh.setTimeout(timeoutMillis);
            if (!Files.isRegularFile(auth.known_hosts_path())) {
                throw new SftpException(SftpError.host_key_rejected, "host key rejected");
            }
            if (Files.size(auth.known_hosts_path()) == 0) {
                throw new SftpException(SftpError.host_key_rejected, "host key rejected");
            }
            ssh.loadKnownHosts(auth.known_hosts_path().toFile());
            ssh.connect(location.host(), location.port());
            authenticate(ssh, location, auth);
            return new SftpSession(ssh, ssh.newSFTPClient());
        } catch (SftpException e) {
            closeQuietly(ssh);
            throw e;
        } catch (IOException e) {
            closeQuietly(ssh);
            throw map(e);
        }
    }

    private static boolean isConnectTimeout(SftpException e) {
        String message = e.getMessage() == null ? "" : e.getMessage().toLowerCase(Locale.ROOT);
        return e.category() == SftpError.io_error && message.contains("connect timed out");
    }

    SFTPClient sftp() {
        return sftp;
    }

    @Override
    public void close() {
        closeQuietly(sftp);
        closeQuietly(ssh);
    }

    static SftpException map(IOException e) {
        if (e instanceof SFTPException sftpException) {
            return mapSftp(sftpException);
        }
        if (e instanceof UserAuthException) {
            return new SftpException(SftpError.authentication_failed, "authentication failed", e);
        }
        String message = e.getMessage() == null ? "" : e.getMessage();
        String lower = message.toLowerCase(Locale.ROOT);
        if (e instanceof TransportException
                && (lower.contains("host key") || lower.contains("known_hosts") || lower.contains("key could not be verified"))) {
            return new SftpException(SftpError.host_key_rejected, "host key rejected", e);
        }
        return new SftpException(SftpError.io_error, message.isBlank() ? "sftp operation failed" : message, e);
    }

    private static void authenticate(SSHClient ssh, SftpLocation location, AuthConfig auth) throws SftpException {
        List<AuthMethod> methods = new ArrayList<>();
        location.password().ifPresent(password -> methods.add(new AuthPassword(
                PasswordUtils.createOneOff(password.toCharArray()))));
        auth.ssh_agent_socket().ifPresent(path -> methods.addAll(agentMethods(path.toString())));
        for (var path : auth.private_key_paths()) {
            if (Files.isRegularFile(path)) {
                try {
                    KeyProvider key = ssh.loadKeys(path.toString());
                    methods.add(new AuthPublickey(key));
                } catch (IOException ignored) {
                }
            }
        }
        if (methods.isEmpty()) {
            throw new SftpException(SftpError.authentication_failed, "authentication failed");
        }
        try {
            ssh.auth(location.user(), methods);
        } catch (UserAuthException e) {
            throw new SftpException(SftpError.authentication_failed, "authentication failed", e);
        } catch (TransportException e) {
            throw new SftpException(SftpError.io_error, "authentication failed", e);
        }
    }

    private static List<AuthMethod> agentMethods(String socketPath) {
        List<AuthMethod> methods = new ArrayList<>();
        try {
            Connector connector = agentConnector(socketPath);
            AgentProxy proxy = new AgentProxy(connector);
            for (Identity identity : proxy.getIdentities()) {
                methods.add(new AuthAgent(proxy, identity));
            }
        } catch (AgentProxyException | Buffer.BufferException | RuntimeException | LinkageError ignored) {
        }
        return methods;
    }

    private static Connector agentConnector(String socketPath) throws AgentProxyException {
        if (socketPath.startsWith("\\\\.\\pipe\\")) {
            return new WindowsNamedPipeAgentConnector(socketPath);
        }
        ConnectorFactory factory = ConnectorFactory.getDefault();
        factory.setPreferredConnectors("ssh-agent");
        factory.setUSocketPath(socketPath);
        return factory.createConnector();
    }

    private static final class WindowsNamedPipeAgentConnector implements Connector {
        private final String pipePath;

        WindowsNamedPipeAgentConnector(String pipePath) throws AgentProxyException {
            this.pipePath = pipePath;
            try (RandomAccessFile ignored = openPipe()) {
            } catch (IOException e) {
                throw new AgentProxyException(e.toString());
            }
        }

        @Override
        public String getName() {
            return "ssh-agent";
        }

        @Override
        public boolean isAvailable() {
            return true;
        }

        @Override
        public void query(com.jcraft.jsch.agentproxy.Buffer buffer) throws AgentProxyException {
            try (RandomAccessFile pipe = openPipe()) {
                pipe.write(buffer.buffer, 0, buffer.getLength());
                buffer.rewind();
                pipe.readFully(buffer.buffer, 0, 4);
                int length = buffer.getInt();
                buffer.rewind();
                buffer.checkFreeSize(length);
                pipe.readFully(buffer.buffer, 0, length);
            } catch (IOException e) {
                throw new AgentProxyException(e.toString());
            }
        }

        private RandomAccessFile openPipe() throws IOException {
            long deadline = System.nanoTime() + 2_000_000_000L;
            while (true) {
                try {
                    return new RandomAccessFile(pipePath, "rw");
                } catch (FileNotFoundException e) {
                    if (System.nanoTime() >= deadline) {
                        throw e;
                    }
                    try {
                        Thread.sleep(20);
                    } catch (InterruptedException interrupted) {
                        Thread.currentThread().interrupt();
                        throw e;
                    }
                }
            }
        }
    }

    private static SftpException mapSftp(SFTPException e) {
        Response.StatusCode code = e.getStatusCode();
        if (code == Response.StatusCode.NO_SUCH_FILE
                || code == Response.StatusCode.NO_SUCH_PATH
                || code == Response.StatusCode.NOT_A_DIRECTORY
                || code == Response.StatusCode.FILE_IS_A_DIRECTORY) {
            return new SftpException(SftpError.not_found, "path not found", e);
        }
        if (code == Response.StatusCode.PERMISSION_DENIED || code == Response.StatusCode.WRITE_PROTECT) {
            return new SftpException(SftpError.permission_denied, "permission denied", e);
        }
        return new SftpException(SftpError.io_error, e.getMessage() == null ? "sftp operation failed" : e.getMessage(), e);
    }

    private static int timeoutMillis(SftpSettings settings) {
        long millis = Math.max(1L, settings.connect_timeout().toMillis());
        return millis > Integer.MAX_VALUE ? Integer.MAX_VALUE : (int) millis;
    }

    private static void silenceSlf4jNoProviderWarning() {
        try {
            Class<?> loggerFactory = Class.forName("org.slf4j.LoggerFactory");
            var state = loggerFactory.getDeclaredField("INITIALIZATION_STATE");
            state.setAccessible(true);
            state.setInt(null, 4);
        } catch (ReflectiveOperationException | RuntimeException ignored) {
        }
    }

    private static void closeQuietly(AutoCloseable closeable) {
        try {
            closeable.close();
        } catch (Exception ignored) {
        }
    }
}
