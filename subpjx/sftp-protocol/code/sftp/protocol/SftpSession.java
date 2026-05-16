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
        SSHClient ssh = new SSHClient();
        try {
            int timeoutMillis = timeoutMillis(settings);
            ssh.setConnectTimeout(timeoutMillis);
            ssh.setTimeout(timeoutMillis);
            if (!Files.isRegularFile(auth.known_hosts_path())) {
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
            ConnectorFactory factory = ConnectorFactory.getDefault();
            factory.setPreferredConnectors("ssh-agent");
            factory.setUSocketPath(socketPath);
            Connector connector = factory.createConnector();
            AgentProxy proxy = new AgentProxy(connector);
            for (Identity identity : proxy.getIdentities()) {
                methods.add(new AuthAgent(proxy, identity));
            }
        } catch (AgentProxyException | Buffer.BufferException | RuntimeException ignored) {
        }
        return methods;
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

    private static void closeQuietly(AutoCloseable closeable) {
        try {
            closeable.close();
        } catch (Exception ignored) {
        }
    }
}
