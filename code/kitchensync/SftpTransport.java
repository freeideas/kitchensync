package kitchensync;

import java.time.Duration;
import java.time.Instant;
import java.util.List;

import sftp.protocol.AuthConfig;
import sftp.protocol.Entry;
import sftp.protocol.PooledSftpFilesystem;
import sftp.protocol.ReadHandle;
import sftp.protocol.SftpConnector;
import sftp.protocol.SftpError;
import sftp.protocol.SftpException;
import sftp.protocol.SftpFilesystem;
import sftp.protocol.SftpLocation;
import sftp.protocol.SftpPoolListener;
import sftp.protocol.SftpPoolRegistry;
import sftp.protocol.SftpSettings;
import sftp.protocol.SftpTransferPool;
import sftp.protocol.WriteHandle;

final class SftpTransport implements Transport {
    private final SftpLocation location;
    private final SftpSettings settings;
    private final AuthConfig authConfig;
    private final SftpPoolRegistry poolRegistry;
    private final SftpPoolListener listener;
    private final boolean pooled;
    private final SftpFilesystem fixed;
    private boolean closed;

    private SftpTransport(SftpLocation location, SftpSettings settings, AuthConfig authConfig,
            SftpPoolRegistry poolRegistry, SftpPoolListener listener, boolean pooled, SftpFilesystem fixed) {
        this.location = location;
        this.settings = settings;
        this.authConfig = authConfig;
        this.poolRegistry = poolRegistry;
        this.listener = listener;
        this.pooled = pooled;
        this.fixed = fixed;
    }

    static SftpTransport connect(SftpParts parts, UrlConfig config, SftpPoolRegistry pools, SftpPoolTrace poolTrace)
            throws TransportException {
        SftpLocation location = new SftpLocation(parts.user(), parts.password(), parts.host(), parts.port(), parts.path());
        SftpSettings settings = new SftpSettings(config.maxConnections(), Duration.ofSeconds(config.connectTimeoutSeconds()),
                Duration.ofSeconds(config.keepAliveSeconds()));
        AuthConfig auth = AuthConfig.defaults();
        ensureRoot(location, settings, auth);
        return new SftpTransport(location, settings, auth, pools, poolTrace::event, false, null);
    }

    private static void ensureRoot(SftpLocation location, SftpSettings settings, AuthConfig auth)
            throws TransportException {
        try (SftpFilesystem fs = SftpConnector.open_unpooled(location, settings, auth)) {
            fs.create_dir("");
            return;
        } catch (SftpException ex) {
            if (ex.category() != SftpError.not_found || location.root_path().equals("/")) {
                throw map(ex);
            }
        }
        SftpLocation root = new SftpLocation(location.user(), location.password(), location.host(), location.port(), "/");
        try (SftpFilesystem fs = SftpConnector.open_unpooled(root, settings, auth)) {
            fs.create_dir(location.root_path().substring(1));
        } catch (SftpException ex) {
            if (rootIsUsable(location, settings, auth)) {
                return;
            }
            throw map(ex);
        }
    }

    private static boolean rootIsUsable(SftpLocation location, SftpSettings settings, AuthConfig auth) {
        try (SftpFilesystem fs = SftpConnector.open_unpooled(location, settings, auth)) {
            fs.create_dir("");
            return true;
        } catch (SftpException ex) {
            return false;
        }
    }

    SftpTransport pooledLease() throws TransportException {
        try {
            SftpLocation poolLocation = new SftpLocation(location.user(), location.password(), location.host(),
                    location.port(), "/");
            SftpTransferPool pool = poolRegistry.pool_for(poolLocation, settings, authConfig, listener);
            PooledSftpFilesystem fs = pool.acquire();
            return new SftpTransport(location, settings, authConfig, poolRegistry, listener, true, fs);
        } catch (SftpException ex) {
            throw map(ex);
        }
    }

    @Override
    public List<EntryInfo> listDir(String relativePath) throws TransportException {
        try (SftpFilesystem fs = open()) {
            List<Entry> entries = fs.list_dir(relativePath(relativePath));
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
        try (SftpFilesystem fs = open()) {
            return entry(fs.stat(relativePath(relativePath)));
        } catch (SftpException ex) {
            throw map(ex);
        }
    }

    @Override
    public ReadToken openRead(String relativePath) throws TransportException {
        try {
            SftpFilesystem fs = open();
            ReadHandle handle = fs.open_read(relativePath(relativePath));
            return new SftpReadToken(fs, handle);
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
            SftpFilesystem fs = open();
            WriteHandle handle = fs.open_write(relativePath(relativePath));
            return new SftpWriteToken(fs, handle);
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
        with(fs -> fs.create_dir(relativePath(relativePath)));
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
        }
    }

    String endpointKey() {
        return location.endpointKey();
    }

    SftpTransport withPoolSettingsFrom(SftpTransport first) {
        SftpSettings poolSettings = new SftpSettings(first.settings.max_connections(), settings.connect_timeout(),
                first.settings.idle_keep_alive_ttl());
        return new SftpTransport(location, poolSettings, authConfig, poolRegistry, listener, false, null);
    }

    private SftpFilesystem open() throws SftpException {
        if (fixed != null) {
            return fixed;
        }
        return SftpConnector.open_unpooled(location, settings, authConfig);
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
            return pooled ? location.root_path().substring(1) : "";
        }
        if (pooled) {
            return PathUtil.child(location.root_path().substring(1), relativePath);
        }
        return relativePath;
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

    private record SftpReadToken(SftpFilesystem fs, ReadHandle handle) implements ReadToken {
        @Override
        public void close() {
            fs.close_read(handle);
            fs.close();
        }
    }

    private record SftpWriteToken(SftpFilesystem fs, WriteHandle handle) implements WriteToken {
        @Override
        public void close() throws TransportException {
            try {
                fs.close_write(handle);
            } catch (SftpException ex) {
                throw map(ex);
            } finally {
                if (!(fs instanceof PooledSftpFilesystem)) {
                    fs.close();
                }
            }
        }
    }
}
