package kitchensync;

import java.time.Duration;
import java.time.Instant;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;

import sftp.protocol.AuthConfig;
import sftp.protocol.Entry;
import sftp.protocol.PooledSftpFilesystem;
import sftp.protocol.ReadHandle;
import sftp.protocol.SftpConnector;
import sftp.protocol.SftpError;
import sftp.protocol.SftpException;
import sftp.protocol.SftpFilesystem;
import sftp.protocol.SftpLocation;
import sftp.protocol.SftpPoolRegistry;
import sftp.protocol.SftpSettings;
import sftp.protocol.SftpTransferPool;
import sftp.protocol.WriteHandle;

final class SftpTransport implements Transport {
    private static final Map<String, Object> STARTUP_LOCKS = new ConcurrentHashMap<>();

    private final SftpLocation location;
    private final SftpSettings settings;
    private final AuthConfig authConfig;
    private final SftpPoolRegistry poolRegistry;
    private final SftpPoolTrace poolTrace;
    private final boolean pooled;
    private final SftpFilesystem fixed;
    private boolean closed;

    private SftpTransport(SftpLocation location, SftpSettings settings, AuthConfig authConfig,
            SftpPoolRegistry poolRegistry, SftpPoolTrace poolTrace, boolean pooled, SftpFilesystem fixed) {
        this.location = location;
        this.settings = settings;
        this.authConfig = authConfig;
        this.poolRegistry = poolRegistry;
        this.poolTrace = poolTrace;
        this.pooled = pooled;
        this.fixed = fixed;
    }

    static SftpTransport connect(SftpParts parts, UrlConfig config, SftpPoolRegistry pools, SftpPoolTrace poolTrace)
            throws TransportException {
        SftpLocation location = new SftpLocation(parts.user(), parts.password(), parts.host(), parts.port(), parts.path());
        SftpSettings settings = new SftpSettings(config.maxConnections(), Duration.ofSeconds(config.connectTimeoutSeconds()),
                Duration.ofSeconds(config.keepAliveSeconds()));
        AuthConfig auth = AuthConfig.defaults();
        SftpLocation root = new SftpLocation(location.user(), location.password(), location.host(), location.port(), "/");
        SftpFilesystem fs;
        try {
            fs = openUnpooled(root, settings, auth);
        } catch (SftpException ex) {
            throw map(ex);
        }
        if (!location.root_path().equals("/")) {
            synchronized (STARTUP_LOCKS.computeIfAbsent(location.endpointKey(), ignored -> new Object())) {
                try {
                    createRootPath(fs, location.root_path().substring(1));
                } catch (SftpException ex) {
                    fs.close();
                    throw map(ex);
                }
            }
        }
        return new SftpTransport(location, settings, auth, pools, poolTrace, false, fs);
    }

    private static void createRootPath(SftpFilesystem fs, String path) throws SftpException {
        String current = "";
        for (String segment : path.split("/")) {
            if (segment.isEmpty()) {
                continue;
            }
            current = current.isEmpty() ? segment : current + "/" + segment;
            try {
                fs.create_dir(current);
            } catch (SftpException ex) {
                if (!isDirectory(fs, current)) {
                    throw ex;
                }
            }
        }
    }

    private static boolean isDirectory(SftpFilesystem fs, String path) {
        try {
            return fs.stat(path).is_dir();
        } catch (SftpException ex) {
            return false;
        }
    }

    SftpTransport pooledLease() throws TransportException {
        String endpoint = location.endpointKey();
        int maximum = settings.max_connections();
        boolean acquiredPermit = false;
        try {
            poolTrace.acquire(endpoint, maximum);
            acquiredPermit = true;
            SftpLocation poolLocation = new SftpLocation(location.user(), location.password(), location.host(),
                    location.port(), "/");
            SftpTransferPool pool = poolRegistry.pool_for(poolLocation, settings, authConfig, event -> {
            });
            PooledSftpFilesystem fs = pooledAcquire(pool, Math.max(1, settings.connect_timeout().toMillis()));
            return new SftpTransport(location, settings, authConfig, poolRegistry, poolTrace, true, fs);
        } catch (InterruptedException ex) {
            Thread.currentThread().interrupt();
            if (acquiredPermit) {
                poolTrace.release(endpoint, maximum);
            }
            throw new TransportException(TransportException.Category.IO_ERROR, "interrupted while waiting for SFTP pool", ex);
        } catch (SftpException ex) {
            if (acquiredPermit) {
                poolTrace.release(endpoint, maximum);
            }
            throw map(ex);
        }
    }

    private static PooledSftpFilesystem pooledAcquire(SftpTransferPool pool, long timeoutMillis)
            throws SftpException, InterruptedException {
        CountDownLatch done = new CountDownLatch(1);
        AtomicBoolean timedOut = new AtomicBoolean(false);
        PooledSftpFilesystem[] result = new PooledSftpFilesystem[1];
        SftpException[] failure = new SftpException[1];
        Thread acquirer = new Thread(() -> {
            try {
                PooledSftpFilesystem fs = pool.acquire();
                if (timedOut.get()) {
                    fs.close();
                } else {
                    result[0] = fs;
                }
            } catch (SftpException ex) {
                if (!timedOut.get()) {
                    failure[0] = ex;
                }
            } finally {
                done.countDown();
            }
        }, "kitchensync-sftp-pool-acquire");
        acquirer.setDaemon(true);
        acquirer.start();
        try {
            if (!done.await(timeoutMillis, TimeUnit.MILLISECONDS)) {
                timedOut.set(true);
                throw new SftpException(SftpError.io_error, "connection timeout");
            }
        } catch (InterruptedException ex) {
            Thread.currentThread().interrupt();
            throw ex;
        }
        if (failure[0] != null) {
            throw failure[0];
        }
        return result[0];
    }

    @Override
    public List<EntryInfo> listDir(String relativePath) throws TransportException {
        try {
            List<Entry> entries = get(fs -> fs.list_dir(relativePath(relativePath)));
            if (entries == null) {
                return List.of();
            }
            return entries.stream()
                    .map(SftpTransport::entry)
                    .toList();
        } catch (SftpException ex) {
            throw map(ex);
        }
    }

    @Override
    public EntryInfo stat(String relativePath) throws TransportException {
        try {
            return entry(get(fs -> fs.stat(relativePath(relativePath))));
        } catch (SftpException ex) {
            throw map(ex);
        }
    }

    @Override
    public ReadToken openRead(String relativePath) throws TransportException {
        try {
            if (fixed != null) {
                return new SftpReadToken(fixed, fixed.open_read(relativePath(relativePath)), false);
            }
            SftpFilesystem fs = open();
            return new SftpReadToken(fs, fs.open_read(relativePath(relativePath)), true);
        } catch (SftpException ex) {
            throw map(ex);
        }
    }

    @Override
    public byte[] read(ReadToken handle, int maxBytes) throws TransportException {
        SftpReadToken token = (SftpReadToken) handle;
        try {
            byte[] chunk = token.fs.read(token.handle, maxBytes);
            return chunk == null ? new byte[0] : chunk;
        } catch (SftpException ex) {
            throw map(ex);
        }
    }

    @Override
    public WriteToken openWrite(String relativePath) throws TransportException {
        try {
            if (fixed != null) {
                return new SftpWriteToken(fixed, fixed.open_write(relativePath(relativePath)), false);
            }
            SftpFilesystem fs = open();
            return new SftpWriteToken(fs, fs.open_write(relativePath(relativePath)), true);
        } catch (SftpException ex) {
            throw map(ex);
        }
    }

    @Override
    public void write(WriteToken handle, byte[] bytes) throws TransportException {
        SftpWriteToken token = (SftpWriteToken) handle;
        try {
            token.fs.write(token.handle, bytes);
        } catch (SftpException ex) {
            throw map(ex);
        }
    }

    @Override
    public void rename(String sourceRelativePath, String targetRelativePath) throws TransportException {
        with(fs -> fs.rename(relativePath(sourceRelativePath), relativePath(targetRelativePath)));
    }

    @Override
    public void deleteFile(String relativePath) throws TransportException {
        with(fs -> fs.delete_file(relativePath(relativePath)));
    }

    @Override
    public void createDir(String relativePath) throws TransportException {
        String path = relativePath(relativePath);
        with(fs -> createRootPath(fs, path));
    }

    @Override
    public void deleteDir(String relativePath) throws TransportException {
        with(fs -> fs.delete_dir(relativePath(relativePath)));
    }

    @Override
    public void setModTime(String relativePath, Instant time) throws TransportException {
        with(fs -> fs.set_mod_time(relativePath(relativePath), time));
    }

    @Override
    public void close() {
        if (fixed != null && !closed) {
            closed = true;
            fixed.close();
            if (pooled) {
                poolTrace.release(location.endpointKey(), settings.max_connections());
            }
        }
    }

    String endpointKey() {
        return location.endpointKey();
    }

    SftpTransport withPoolSettingsFrom(SftpTransport first) {
        SftpSettings poolSettings = new SftpSettings(first.settings.max_connections(), settings.connect_timeout(),
                first.settings.idle_keep_alive_ttl());
        return new SftpTransport(location, poolSettings, authConfig, poolRegistry, poolTrace, false, null);
    }

    private SftpFilesystem open() throws SftpException {
        if (fixed != null) {
            return fixed;
        }
        SftpLocation root = new SftpLocation(location.user(), location.password(), location.host(), location.port(), "/");
        return openUnpooled(root, settings, authConfig);
    }

    private static SftpFilesystem openUnpooled(SftpLocation location, SftpSettings settings, AuthConfig auth)
            throws SftpException {
        CountDownLatch done = new CountDownLatch(1);
        AtomicBoolean timedOut = new AtomicBoolean(false);
        SftpFilesystem[] result = new SftpFilesystem[1];
        SftpException[] failure = new SftpException[1];
        Thread connector = new Thread(() -> {
            try {
                SftpFilesystem fs = SftpConnector.open_unpooled(location, settings, auth);
                if (timedOut.get()) {
                    fs.close();
                } else {
                    result[0] = fs;
                }
            } catch (SftpException ex) {
                failure[0] = ex;
            } finally {
                done.countDown();
            }
        }, "kitchensync-sftp-connect");
        connector.setDaemon(true);
        connector.start();
        try {
            if (!done.await(Math.max(1, settings.connect_timeout().toMillis()), TimeUnit.MILLISECONDS)) {
                timedOut.set(true);
                throw new SftpException(SftpError.io_error, "connection timeout");
            }
        } catch (InterruptedException ex) {
            Thread.currentThread().interrupt();
            throw new SftpException(SftpError.io_error, "connection interrupted", ex);
        }
        if (failure[0] != null) {
            throw failure[0];
        }
        return result[0];
    }

    private <T> T get(SftpGetter<T> op) throws SftpException {
        if (fixed != null) {
            return op.run(fixed);
        }
        try (SftpFilesystem fs = open()) {
            return op.run(fs);
        }
    }

    private void with(SftpOperation operation) throws TransportException {
        if (fixed != null) {
            try {
                operation.run(fixed);
                return;
            } catch (SftpException ex) {
                throw map(ex);
            }
        }
        try (SftpFilesystem fs = open()) {
            operation.run(fs);
        } catch (SftpException ex) {
            throw map(ex);
        }
    }

    private String relativePath(String relativePath) {
        if (relativePath == null || relativePath.isEmpty()) {
            return location.root_path().substring(1);
        }
        return PathUtil.child(location.root_path().substring(1), relativePath);
    }

    private static EntryInfo entry(Entry entry) {
        return new EntryInfo(entry.name(), entry.is_dir(), entry.mod_time(), entry.byte_size());
    }

    private static TransportException map(SftpException ex) {
        TransportException.Category category = switch (ex.category()) {
            case not_found -> TransportException.Category.NOT_FOUND;
            case permission_denied -> TransportException.Category.PERMISSION_DENIED;
            default -> TransportException.Category.IO_ERROR;
        };
        return new TransportException(category, ex.getMessage(), ex);
    }

    private interface SftpOperation {
        void run(SftpFilesystem fs) throws SftpException;
    }

    private interface SftpGetter<T> {
        T run(SftpFilesystem fs) throws SftpException;
    }

    private record SftpReadToken(SftpFilesystem fs, ReadHandle handle, boolean ownsFs) implements ReadToken {
        @Override
        public void close() {
            fs.close_read(handle);
            if (ownsFs) {
                fs.close();
            }
        }
    }

    private record SftpWriteToken(SftpFilesystem fs, WriteHandle handle, boolean ownsFs) implements WriteToken {
        @Override
        public void close() throws TransportException {
            try {
                fs.close_write(handle);
            } catch (SftpException ex) {
                throw map(ex);
            } finally {
                if (ownsFs) {
                    fs.close();
                }
            }
        }
    }
}
