package sftp.protocol;

import connection.pool.Pool;
import connection.pool.PoolSettings;
import connection.pool.Pools;
import ssh.sftp.session.Credential;
import ssh.sftp.session.Entry;
import ssh.sftp.session.Failure;
import ssh.sftp.session.ReadHandle;
import ssh.sftp.session.Session;
import ssh.sftp.session.SftpFailureException;
import ssh.sftp.session.SshSftp;
import ssh.sftp.session.WriteHandle;

import java.io.IOException;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;

public final class Transport {

    public static final class PoolGroup {
        final String user;
        final String host;
        final int mc;
        volatile int port;
        volatile String password;
        Pool<SessionContext> pool;
        final AtomicInteger inUse = new AtomicInteger();
        volatile boolean shutdown = false;

        PoolGroup(String user, String host, int mc) {
            this.user = user;
            this.host = host;
            this.mc = mc;
        }
    }

    public static final class SessionContext {
        final String connId;
        final Session session;
        final PoolGroup group;

        SessionContext(String connId, Session session, PoolGroup group) {
            this.connId = connId;
            this.session = session;
            this.group = group;
        }
    }

    public static final class EndpointHandle {
        public final String endpointId;
        public final PoolGroup group;

        EndpointHandle(String endpointId, PoolGroup group) {
            this.endpointId = endpointId;
            this.group = group;
        }
    }

    private final Pools poolRegistry = new Pools();
    private final Map<String, PoolGroup> poolByKey = new ConcurrentHashMap<>();
    private final Map<String, EndpointHandle> endpoints = new ConcurrentHashMap<>();
    private final Map<String, SessionContext> connections = new ConcurrentHashMap<>();
    private final Map<String, ReadHandle> reads = new ConcurrentHashMap<>();
    private final Map<String, WriteHandle> writes = new ConcurrentHashMap<>();
    private final AtomicLong epSeq = new AtomicLong();
    private final boolean trace = "trace".equalsIgnoreCase(System.getenv("VERBOSITY"));

    public String openEndpoint(String user, String host, Integer port, String password, int mc, int ct, int ka) {
        int effPort = (port == null) ? 22 : port;
        String key = user + "@" + host;
        PoolGroup g = poolByKey.compute(key, (k, existing) -> {
            if (existing != null && !existing.shutdown) return existing;
            PoolGroup pg = new PoolGroup(user, host, mc);
            pg.port = effPort;
            pg.password = password;
            PoolSettings ps = new PoolSettings(mc, ct, ka);
            // Use a fresh key Object so Pools.register creates a new Pool
            // even if a prior pool for the same (user,host) was shut down.
            Object regKey = (existing == null) ? k : new Object();
            pg.pool = poolRegistry.register(
                    regKey,
                    () -> openSessionFor(pg, ct),
                    ctx -> closeSessionFor(ctx),
                    ps,
                    null);
            return pg;
        });
        String epId = "ep-" + epSeq.incrementAndGet();
        endpoints.put(epId, new EndpointHandle(epId, g));
        return epId;
    }

    private SessionContext openSessionFor(PoolGroup g, int ct) {
        List<Credential> creds = Auth.build(g.password);
        try {
            Session s = SshSftp.openSession(g.host, g.port, g.user, creds, ct);
            String connId = "conn-" + UUID.randomUUID();
            SessionContext ctx = new SessionContext(connId, s, g);
            connections.put(connId, ctx);
            return ctx;
        } catch (SftpFailureException e) {
            throw new RuntimeException("io_failure: " + e.failure.code());
        } catch (RuntimeException e) {
            throw new RuntimeException("io_failure: " + e.getMessage(), e);
        }
    }

    private void closeSessionFor(SessionContext ctx) {
        try {
            SshSftp.closeSession(ctx.session);
        } catch (RuntimeException ignored) {
            // closing best-effort
        }
        connections.remove(ctx.connId);
    }

    public String acquire(String epId) throws IOException {
        EndpointHandle h = endpoints.get(epId);
        if (h == null) throw new IOException("unknown endpoint: " + epId);
        SessionContext ctx;
        try {
            ctx = h.group.pool.acquire();
        } catch (RuntimeException e) {
            Throwable c = e;
            while (c.getCause() != null && c.getCause() != c) c = c.getCause();
            throw new IOException("I/O error: " + c.getMessage());
        }
        int inUse = h.group.inUse.incrementAndGet();
        if (trace) {
            System.err.println("endpoint=" + h.group.user + "@" + h.group.host
                    + " connections=" + inUse + "/" + h.group.mc);
        }
        return ctx.connId;
    }

    public void release(String connId) {
        SessionContext ctx = connections.get(connId);
        if (ctx == null) return;
        int newInUse;
        try {
            ctx.group.pool.release(ctx);
        } finally {
            int prev;
            do {
                prev = ctx.group.inUse.get();
                if (prev <= 0) { newInUse = 0; break; }
                newInUse = prev - 1;
            } while (!ctx.group.inUse.compareAndSet(prev, newInUse));
        }
        if (trace) {
            System.err.println("endpoint=" + ctx.group.user + "@" + ctx.group.host
                    + " connections=" + newInUse + "/" + ctx.group.mc);
        }
    }

    public void closeEndpoint(String epId) {
        EndpointHandle h = endpoints.get(epId);
        if (h == null) return;
        h.group.shutdown = true;
        h.group.pool.closePool();
    }

    private SessionContext requireConn(String connId) throws IOException {
        SessionContext ctx = connections.get(connId);
        if (ctx == null) throw new IOException("unknown connection: " + connId);
        return ctx;
    }

    public List<Map<String, Object>> listDir(String connId, String path) throws IOException, SftpFailureException {
        SessionContext ctx = requireConn(connId);
        List<Entry> entries = SshSftp.listDir(ctx.session, path);
        List<Map<String, Object>> out = new ArrayList<>();
        for (Entry e : entries) {
            Map<String, Object> m = new LinkedHashMap<>();
            m.put("name", e.name());
            m.put("is_dir", e.isDir());
            m.put("mod_time", e.modTime());
            m.put("byte_size", e.isDir() ? -1L : e.byteSize());
            out.add(m);
        }
        return out;
    }

    public SshSftp.StatResult stat(String connId, String path) throws IOException, SftpFailureException {
        SessionContext ctx = requireConn(connId);
        return SshSftp.stat(ctx.session, path);
    }

    public String openRead(String connId, String path) throws IOException, SftpFailureException {
        SessionContext ctx = requireConn(connId);
        ReadHandle h = SshSftp.openRead(ctx.session, path);
        reads.put(h.id, h);
        return h.id;
    }

    public byte[] read(String handleId, int max) throws IOException {
        ReadHandle h = reads.get(handleId);
        if (h == null) throw new IOException("unknown read handle: " + handleId);
        return SshSftp.read(h, max);
    }

    public boolean atEof(String handleId) throws IOException {
        ReadHandle h = reads.get(handleId);
        if (h == null) throw new IOException("unknown read handle: " + handleId);
        return SshSftp.atEof(h);
    }

    public void closeRead(String handleId) {
        ReadHandle h = reads.remove(handleId);
        if (h != null) SshSftp.closeRead(h);
    }

    public String openWrite(String connId, String path) throws IOException, SftpFailureException {
        SessionContext ctx = requireConn(connId);
        WriteHandle h = SshSftp.openWrite(ctx.session, path);
        writes.put(h.id, h);
        return h.id;
    }

    public void write(String handleId, byte[] bytes) throws IOException {
        WriteHandle h = writes.get(handleId);
        if (h == null) throw new IOException("unknown write handle: " + handleId);
        SshSftp.write(h, bytes);
    }

    public void closeWrite(String handleId) throws IOException, SftpFailureException {
        WriteHandle h = writes.remove(handleId);
        if (h == null) throw new IOException("unknown write handle: " + handleId);
        SshSftp.closeWrite(h);
    }

    public void rename(String connId, String src, String dst) throws IOException, SftpFailureException {
        SessionContext ctx = requireConn(connId);
        SshSftp.rename(ctx.session, src, dst);
    }

    public void deleteFile(String connId, String path) throws IOException, SftpFailureException {
        SessionContext ctx = requireConn(connId);
        SshSftp.deleteFile(ctx.session, path);
    }

    public void deleteDir(String connId, String path) throws IOException, SftpFailureException {
        SessionContext ctx = requireConn(connId);
        SshSftp.deleteDir(ctx.session, path);
    }

    public void createDir(String connId, String path) throws IOException, SftpFailureException {
        SessionContext ctx = requireConn(connId);
        SshSftp.createDir(ctx.session, path);
    }

    public void setModTime(String connId, String path, long time) throws IOException, SftpFailureException {
        SessionContext ctx = requireConn(connId);
        SshSftp.setModTime(ctx.session, path, time);
    }

    public static String failureCode(Failure f) {
        return switch (f) {
            case NOT_FOUND -> "not_found";
            case PERMISSION_DENIED -> "permission_denied";
            case IO_FAILURE -> "io_failure";
        };
    }
}
