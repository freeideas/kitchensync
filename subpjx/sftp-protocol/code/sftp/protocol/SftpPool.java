package sftp.protocol;

import bounded.keyed.pool.BoundedKeyedPool;
import bounded.keyed.pool.Handle;
import org.apache.sshd.agent.SshAgent;
import org.apache.sshd.client.config.hosts.KnownHostEntry;
import org.apache.sshd.client.SshClient;
import org.apache.sshd.client.auth.password.UserAuthPasswordFactory;
import org.apache.sshd.client.auth.pubkey.UserAuthPublicKeyFactory;
import org.apache.sshd.client.future.ConnectFuture;
import org.apache.sshd.client.session.ClientSession;
import org.apache.sshd.common.config.keys.KeyUtils;
import org.apache.sshd.common.config.keys.PublicKeyEntryResolver;
import org.apache.sshd.common.keyprovider.FileKeyPairProvider;
import org.apache.sshd.common.keyprovider.KeyIdentityProvider;
import org.apache.sshd.common.signature.BuiltinSignatures;
import org.apache.sshd.sftp.client.SftpClient;
import org.apache.sshd.sftp.client.SftpClientFactory;

import java.io.IOException;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.SocketAddress;
import java.net.URI;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.security.GeneralSecurityException;
import java.security.KeyPair;
import java.security.PublicKey;
import java.util.Arrays;
import java.util.concurrent.TimeUnit;

public final class SftpPool {
    private final SftpPoolConfig config;
    private final BoundedKeyedPool<PoolKey, SftpSession> pool;
    private final SshClient sshClient;
    private final ThreadLocal<String> pendingPassword = new ThreadLocal<>();

    public SftpPool(SftpPoolConfig config) {
        this.config = config;
        this.sshClient = buildSshClient();
        this.pool = new BoundedKeyedPool<>(
            key -> createSession(key),
            SftpSession::close,
            config.maxConnections(),
            config.idleKeepaliveSeconds()
        );
        sshClient.start();
    }

    private SshClient buildSshClient() {
        SshClient client = SshClient.setUpDefaultClient();

        Path knownHosts = Paths.get(System.getProperty("user.home"), ".ssh", "known_hosts");
        client.setServerKeyVerifier((session, address, key) ->
            isKnownHostKey(session, address, key, knownHosts)
        );

        client.setUserAuthFactories(Arrays.asList(
            UserAuthPasswordFactory.INSTANCE,
            UserAuthPublicKeyFactory.INSTANCE
        ));
        client.setKeyIdentityProvider(KeyIdentityProvider.EMPTY_KEYS_PROVIDER);
        client.setSignatureFactories(Arrays.asList(
            BuiltinSignatures.ed25519,
            BuiltinSignatures.nistp256,
            BuiltinSignatures.nistp384,
            BuiltinSignatures.nistp521,
            BuiltinSignatures.rsaSHA512,
            BuiltinSignatures.rsaSHA256,
            BuiltinSignatures.rsa
        ));

        String agentSocket = System.getenv("SSH_AUTH_SOCK");
        if (agentSocket != null && !agentSocket.isBlank()) {
            try {
                client.getProperties().put(SshAgent.SSH_AUTHSOCKET_ENV_NAME, agentSocket);
                client.setAgentFactory(new OpenSshAgentFactory());
            } catch (Exception ignored) {}
        }

        return client;
    }

    private static boolean isKnownHostKey(
        ClientSession session,
        SocketAddress address,
        PublicKey serverKey,
        Path knownHosts
    ) {
        if (!Files.exists(knownHosts)) return false;
        try {
            HostAddress hostAddress = HostAddress.from(address);
            for (KnownHostEntry entry : KnownHostEntry.readKnownHostEntries(knownHosts)) {
                if (!hostAddress.matches(entry)) continue;
                PublicKey knownKey = entry.getKeyEntry()
                    .resolvePublicKey(session, PublicKeyEntryResolver.FAILING);
                if (KeyUtils.compareKeys(serverKey, knownKey)) return true;
            }
        } catch (IOException | GeneralSecurityException | RuntimeException e) {
            return false;
        }
        return false;
    }

    private record HostAddress(String hostName, String hostAddress, int port) {
        static HostAddress from(SocketAddress address) {
            if (address instanceof InetSocketAddress inet) {
                InetAddress resolved = inet.getAddress();
                return new HostAddress(
                    inet.getHostString(),
                    resolved == null ? null : resolved.getHostAddress(),
                    inet.getPort()
                );
            }
            return new HostAddress(address.toString(), null, 22);
        }

        boolean matches(KnownHostEntry entry) {
            return entry.isHostMatch(hostName, port)
                || (hostAddress != null && entry.isHostMatch(hostAddress, port));
        }
    }

    private SftpSession createSession(PoolKey key) {
        String password = pendingPassword.get();
        long timeoutMs = (long) (config.connectTimeoutSeconds() * 1000);
        try {
            ConnectFuture connectFuture = sshClient.connect(key.user(), key.host(), key.port());
            if (!connectFuture.await(timeoutMs, TimeUnit.MILLISECONDS)) {
                throw new SftpIoException("connection timed out to " + key.host());
            }
            if (!connectFuture.isConnected()) {
                Throwable cause = connectFuture.getException();
                throw new SftpIoException("connection failed to " + key.host() +
                    (cause != null ? ": " + cause.getMessage() : ""), cause instanceof Exception ex ? ex : null);
            }
            ClientSession session = connectFuture.getSession();

            boolean authenticated = false;
            if (password != null) {
                session.setUserAuthFactories(Arrays.asList(UserAuthPasswordFactory.INSTANCE));
                session.addPasswordIdentity(password);
                authenticated = authenticate(session, timeoutMs);
                session.removePasswordIdentity(password);
            }

            String agentSocket = System.getenv("SSH_AUTH_SOCK");
            if (!authenticated && agentSocket != null && !agentSocket.isBlank()) {
                session.setUserAuthFactories(Arrays.asList(UserAuthPublicKeyFactory.INSTANCE));
                authenticated = authenticate(session, timeoutMs);
            }

            Path sshDir = Paths.get(System.getProperty("user.home"), ".ssh");
            for (String keyName : new String[]{"id_ed25519", "id_ecdsa", "id_rsa"}) {
                if (authenticated) break;
                Path keyFile = sshDir.resolve(keyName);
                if (Files.exists(keyFile)) {
                    try {
                        FileKeyPairProvider provider = new FileKeyPairProvider(keyFile);
                        for (KeyPair kp : provider.loadKeys(session)) {
                            session.setUserAuthFactories(Arrays.asList(UserAuthPublicKeyFactory.INSTANCE));
                            session.addPublicKeyIdentity(kp);
                            authenticated = authenticate(session, timeoutMs);
                            session.removePublicKeyIdentity(kp);
                            if (authenticated) break;
                        }
                    } catch (Exception ignored) {}
                }
            }

            if (!authenticated) {
                session.close(true);
                throw new SftpIoException("authentication failed for " + key.user() + "@" + key.host());
            }

            SftpClient sftpClient = SftpClientFactory.instance().createSftpClient(session);
            return new SftpSession(session, sftpClient);
        } catch (SftpIoException e) {
            throw e;
        } catch (Exception e) {
            throw new SftpIoException("failed to connect to " + key.host() + ": " + e.getMessage(), e);
        }
    }

    private static boolean authenticate(ClientSession session, long timeoutMs) {
        try {
            return session.auth().verify(timeoutMs, TimeUnit.MILLISECONDS).isSuccess();
        } catch (Exception e) {
            return false;
        }
    }

    public ConnectionHandle acquire(String url) throws InterruptedException {
        URI uri = URI.create(url);
        String userInfo = uri.getUserInfo();
        String user;
        String password = null;
        if (userInfo != null && !userInfo.isEmpty()) {
            int colon = userInfo.indexOf(':');
            if (colon >= 0) {
                user = userInfo.substring(0, colon);
                password = userInfo.substring(colon + 1);
            } else {
                user = userInfo;
            }
        } else {
            user = System.getProperty("user.name");
        }
        String host = uri.getHost();
        int port = uri.getPort() > 0 ? uri.getPort() : 22;

        PoolKey key = new PoolKey(user, host, port);
        pendingPassword.set(password);
        try {
            Handle<PoolKey, SftpSession> handle = pool.acquire(key);
            return new ConnectionHandle(handle, pool);
        } finally {
            pendingPassword.remove();
        }
    }

    public void release(ConnectionHandle handle) {
        pool.release(handle.poolHandle());
    }

    public void shutdown() {
        pool.shutdown();
        try { sshClient.stop(); } catch (Exception ignored) {}
    }
}
