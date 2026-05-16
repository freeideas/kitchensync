package sftp.protocol;

import java.util.HashMap;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;

public final class SftpPoolRegistry implements AutoCloseable {
    private final Map<String, SftpTransferPool> pools = new HashMap<>();
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
        return pools.computeIfAbsent(
                location.endpointKey(),
                key -> new SftpTransferPool(location, settings, auth_config, Optional.ofNullable(pool_listener)));
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
        for (SftpTransferPool pool : pools.values()) {
            pool.close();
        }
        pools.clear();
    }
}
