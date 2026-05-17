package sftp.protocol;

import bounded.resource.pool.BoundedPool;
import bounded.resource.pool.BoundedPoolRegistry;
import bounded.resource.pool.PoolSettings;
import bounded.resource.pool.ResourceFactory;

import java.util.HashMap;
import java.util.Map;
import java.util.Objects;

public final class SftpPoolRegistry implements AutoCloseable {
    private final BoundedPoolRegistry<String, SftpSession> delegate = new BoundedPoolRegistry<>();
    private final Map<String, SftpTransferPool> wrappers = new HashMap<>();
    private boolean closed;

    public synchronized SftpTransferPool pool_for(
            SftpLocation location,
            SftpSettings settings,
            AuthConfig auth_config,
            SftpPoolListener pool_listener) {
        Objects.requireNonNull(location, "location");
        Objects.requireNonNull(settings, "settings");
        Objects.requireNonNull(auth_config, "auth_config");
        if (closed) {
            throw new IllegalStateException("registry is closed");
        }
        String key = location.endpointKey();
        BoundedPool<SftpSession> pool = delegate.pool_for(
                key,
                new PoolSettings(settings.max_connections(), settings.idle_keep_alive_ttl()),
                factory(location, settings, auth_config),
                pool_listener == null ? null : event -> pool_listener.on_event(new PoolEvent(
                        event.key(),
                        event.open_resources(),
                        event.max_resources())));
        return wrappers.computeIfAbsent(key, ignored -> new SftpTransferPool(location, pool));
    }

    public synchronized SftpTransferPool pool_for(
            SftpLocation location,
            SftpSettings settings,
            AuthConfig auth_config) {
        return pool_for(location, settings, auth_config, null);
    }

    @Override
    public synchronized void close() {
        if (closed) {
            return;
        }
        closed = true;
        delegate.close();
        wrappers.clear();
    }

    private ResourceFactory<String, SftpSession> factory(
            SftpLocation location,
            SftpSettings settings,
            AuthConfig auth) {
        return new ResourceFactory<>() {
            @Override
            public SftpSession open(String ignored) throws SftpException {
                return SftpSession.open(location, settings, auth);
            }

            @Override
            public void close(SftpSession session) {
                session.close();
            }
        };
    }
}
