package snapshot.db;

import java.nio.charset.StandardCharsets;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Statement;
import java.time.Instant;
import java.time.ZoneOffset;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class SnapshotDb implements AutoCloseable {

    private static final long PRIME64_1 = 0x9E3779B185EBCA87L;
    private static final long PRIME64_2 = 0xC2B2AE3D27D4EB4FL;
    private static final long PRIME64_3 = 0x165667B19E3779F9L;
    private static final long PRIME64_4 = 0x85EBCA77C2B2AE63L;
    private static final long PRIME64_5 = 0x27D4EB2F165667C5L;

    private static final String B62 =
        "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

    private static final DateTimeFormatter TS_DATE_FMT =
        DateTimeFormatter.ofPattern("yyyy-MM-dd_HH-mm-ss").withZone(ZoneOffset.UTC);

    public static final String ROOT_SENTINEL = computeHash("/");

    private final Connection conn;
    private long lastMicros = 0L;

    private SnapshotDb(Connection conn) { this.conn = conn; }

    public static SnapshotDb open(String path) throws SQLException {
        Connection c = DriverManager.getConnection("jdbc:sqlite:" + path);
        c.setAutoCommit(true);
        try (Statement s = c.createStatement()) {
            s.execute("PRAGMA journal_mode=WAL");
            s.execute("PRAGMA foreign_keys=ON");
            s.execute(
                "CREATE TABLE IF NOT EXISTS snapshot (" +
                "  id TEXT PRIMARY KEY, " +
                "  parent_id TEXT NOT NULL, " +
                "  basename TEXT NOT NULL, " +
                "  mod_time TEXT NOT NULL, " +
                "  byte_size INTEGER NOT NULL, " +
                "  last_seen TEXT, " +
                "  deleted_time TEXT, " +
                "  FOREIGN KEY (parent_id) REFERENCES snapshot(id)" +
                ")");
            s.execute("CREATE INDEX IF NOT EXISTS idx_snapshot_parent_id ON snapshot(parent_id)");
            s.execute("CREATE INDEX IF NOT EXISTS idx_snapshot_last_seen ON snapshot(last_seen)");
            s.execute("CREATE INDEX IF NOT EXISTS idx_snapshot_deleted_time ON snapshot(deleted_time)");
        }
        insertRootSentinel(c);
        return new SnapshotDb(c);
    }

    private static void insertRootSentinel(Connection c) throws SQLException {
        try (PreparedStatement ps = c.prepareStatement(
                "INSERT OR IGNORE INTO snapshot " +
                "(id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) " +
                "VALUES (?, ?, ?, ?, ?, NULL, NULL)")) {
            ps.setString(1, ROOT_SENTINEL);
            ps.setString(2, ROOT_SENTINEL);
            ps.setString(3, "/");
            ps.setString(4, "1970-01-01_00-00-00_000000Z");
            ps.setLong(5, -1L);
            ps.executeUpdate();
        }
    }

    @Override
    public void close() throws SQLException {
        try (Statement s = conn.createStatement()) {
            s.execute("PRAGMA wal_checkpoint(TRUNCATE)");
        } catch (SQLException ignored) {}
        conn.close();
    }

    // ---- path hashing ----

    public static String hashPath(String path) {
        return computeHash(path);
    }

    private static String computeHash(String path) {
        byte[] bytes = path.getBytes(StandardCharsets.UTF_8);
        long h = xxh64(bytes, 0L);
        return base62(h);
    }

    private static long xxh64(byte[] data, long seed) {
        int len = data.length;
        long h64;
        int p = 0;
        if (len >= 32) {
            long v1 = seed + PRIME64_1 + PRIME64_2;
            long v2 = seed + PRIME64_2;
            long v3 = seed;
            long v4 = seed - PRIME64_1;
            int limit = len - 32;
            while (p <= limit) {
                v1 = round(v1, readLongLE(data, p)); p += 8;
                v2 = round(v2, readLongLE(data, p)); p += 8;
                v3 = round(v3, readLongLE(data, p)); p += 8;
                v4 = round(v4, readLongLE(data, p)); p += 8;
            }
            h64 = rotl(v1, 1) + rotl(v2, 7) + rotl(v3, 12) + rotl(v4, 18);
            h64 = mergeRound(h64, v1);
            h64 = mergeRound(h64, v2);
            h64 = mergeRound(h64, v3);
            h64 = mergeRound(h64, v4);
        } else {
            h64 = seed + PRIME64_5;
        }
        h64 += len;
        while (p + 8 <= len) {
            long k1 = round(0L, readLongLE(data, p));
            h64 ^= k1;
            h64 = rotl(h64, 27) * PRIME64_1 + PRIME64_4;
            p += 8;
        }
        while (p + 4 <= len) {
            h64 ^= (readIntLE(data, p) & 0xFFFFFFFFL) * PRIME64_1;
            h64 = rotl(h64, 23) * PRIME64_2 + PRIME64_3;
            p += 4;
        }
        while (p < len) {
            h64 ^= (data[p] & 0xFFL) * PRIME64_5;
            h64 = rotl(h64, 11) * PRIME64_1;
            p++;
        }
        h64 ^= h64 >>> 33;
        h64 *= PRIME64_2;
        h64 ^= h64 >>> 29;
        h64 *= PRIME64_3;
        h64 ^= h64 >>> 32;
        return h64;
    }

    private static long round(long acc, long input) {
        acc += input * PRIME64_2;
        acc = rotl(acc, 31);
        acc *= PRIME64_1;
        return acc;
    }

    private static long mergeRound(long acc, long val) {
        val = round(0L, val);
        acc ^= val;
        return acc * PRIME64_1 + PRIME64_4;
    }

    private static long rotl(long v, int n) {
        return (v << n) | (v >>> (64 - n));
    }

    private static long readLongLE(byte[] d, int p) {
        return  (d[p]     & 0xFFL)
             | ((d[p + 1] & 0xFFL) << 8)
             | ((d[p + 2] & 0xFFL) << 16)
             | ((d[p + 3] & 0xFFL) << 24)
             | ((d[p + 4] & 0xFFL) << 32)
             | ((d[p + 5] & 0xFFL) << 40)
             | ((d[p + 6] & 0xFFL) << 48)
             | ((d[p + 7] & 0xFFL) << 56);
    }

    private static int readIntLE(byte[] d, int p) {
        return  (d[p]     & 0xFF)
             | ((d[p + 1] & 0xFF) << 8)
             | ((d[p + 2] & 0xFF) << 16)
             | ((d[p + 3] & 0xFF) << 24);
    }

    private static String base62(long h) {
        char[] out = new char[11];
        long n = h;
        for (int i = 10; i >= 0; i--) {
            int d = (int) Long.remainderUnsigned(n, 62L);
            out[i] = B62.charAt(d);
            n = Long.divideUnsigned(n, 62L);
        }
        return new String(out);
    }

    // ---- timestamps ----

    public synchronized String currentTimestamp() {
        Instant now = Instant.now();
        long micros = now.getEpochSecond() * 1_000_000L + now.getNano() / 1_000L;
        if (micros <= lastMicros) micros = lastMicros + 1;
        lastMicros = micros;
        long secs = Math.floorDiv(micros, 1_000_000L);
        long us   = Math.floorMod(micros, 1_000_000L);
        String prefix = TS_DATE_FMT.format(Instant.ofEpochSecond(secs));
        return prefix + "_" + String.format("%06d", us) + "Z";
    }

    // ---- row ops ----

    public Map<String, Object> lookupRow(String id) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement(
                "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time " +
                "FROM snapshot WHERE id = ?")) {
            ps.setString(1, id);
            try (ResultSet rs = ps.executeQuery()) {
                if (!rs.next()) return null;
                return rowToMap(rs);
            }
        }
    }

    public List<Map<String, Object>> listChildren(String parentId) throws SQLException {
        List<Map<String, Object>> rows = new ArrayList<>();
        try (PreparedStatement ps = conn.prepareStatement(
                "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time " +
                "FROM snapshot WHERE parent_id = ? AND id != parent_id")) {
            ps.setString(1, parentId);
            try (ResultSet rs = ps.executeQuery()) {
                while (rs.next()) rows.add(rowToMap(rs));
            }
        }
        return rows;
    }

    private static Map<String, Object> rowToMap(ResultSet rs) throws SQLException {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("id", rs.getString("id"));
        m.put("parent_id", rs.getString("parent_id"));
        m.put("basename", rs.getString("basename"));
        m.put("mod_time", rs.getString("mod_time"));
        m.put("byte_size", rs.getLong("byte_size"));
        String ls = rs.getString("last_seen");
        m.put("last_seen", rs.wasNull() ? null : ls);
        String dt = rs.getString("deleted_time");
        m.put("deleted_time", rs.wasNull() ? null : dt);
        return m;
    }

    public void upsertConfirmedStrict(String path, String basename, String modTime,
                                       long byteSize, String lastSeen) throws SQLException {
        writeConfirmedRow(path, basename, modTime, byteSize, lastSeen);
    }

    public void upsertConfirmed(String path, String basename, String modTime,
                                 long byteSize, String lastSeen) throws SQLException {
        ensureParentChain(path);
        writeConfirmedRow(path, basename, modTime, byteSize, lastSeen);
    }

    private void writeConfirmedRow(String path, String basename, String modTime,
                                    long byteSize, String lastSeen) throws SQLException {
        String id = hashPath(path);
        String parentId = parentPathId(path);
        try (PreparedStatement ps = conn.prepareStatement(
                "INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) " +
                "VALUES (?, ?, ?, ?, ?, ?, NULL) " +
                "ON CONFLICT(id) DO UPDATE SET " +
                "  parent_id=excluded.parent_id, basename=excluded.basename, " +
                "  mod_time=excluded.mod_time, byte_size=excluded.byte_size, " +
                "  last_seen=excluded.last_seen, deleted_time=NULL")) {
            ps.setString(1, id);
            ps.setString(2, parentId);
            ps.setString(3, basename);
            ps.setString(4, modTime);
            ps.setLong(5, byteSize);
            ps.setString(6, lastSeen);
            ps.executeUpdate();
        }
    }

    public void upsertUnconfirmed(String path, String basename, String modTime,
                                   long byteSize) throws SQLException {
        ensureParentChain(path);
        String id = hashPath(path);
        String parentId = parentPathId(path);
        try (PreparedStatement ps = conn.prepareStatement(
                "INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) " +
                "VALUES (?, ?, ?, ?, ?, NULL, NULL) " +
                "ON CONFLICT(id) DO UPDATE SET " +
                "  parent_id=excluded.parent_id, basename=excluded.basename, " +
                "  mod_time=excluded.mod_time, byte_size=excluded.byte_size, " +
                "  deleted_time=NULL")) {
            ps.setString(1, id);
            ps.setString(2, parentId);
            ps.setString(3, basename);
            ps.setString(4, modTime);
            ps.setLong(5, byteSize);
            ps.executeUpdate();
        }
    }

    public void markCopyCompleted(String path, String ts) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement(
                "UPDATE snapshot SET last_seen = ? WHERE id = ?")) {
            ps.setString(1, ts);
            ps.setString(2, hashPath(path));
            ps.executeUpdate();
        }
    }

    public void markAbsent(String path) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement(
                "UPDATE snapshot SET deleted_time = last_seen " +
                "WHERE id = ? AND deleted_time IS NULL")) {
            ps.setString(1, hashPath(path));
            ps.executeUpdate();
        }
    }

    public void cascadeTombstone(String id, String timestamp) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement(
                "WITH RECURSIVE descendants(id) AS (" +
                "  SELECT id FROM snapshot WHERE parent_id = ? AND id != parent_id " +
                "  UNION " +
                "  SELECT s.id FROM snapshot s, descendants d " +
                "    WHERE s.parent_id = d.id AND s.id != s.parent_id" +
                ") " +
                "UPDATE snapshot SET deleted_time = ? " +
                "WHERE id IN (SELECT id FROM descendants) AND deleted_time IS NULL")) {
            ps.setString(1, id);
            ps.setString(2, timestamp);
            ps.executeUpdate();
        }
    }

    public void purge(String cutoff) throws SQLException {
        try (PreparedStatement ps = conn.prepareStatement(
                "DELETE FROM snapshot WHERE id != ? AND (" +
                "  (deleted_time IS NOT NULL AND deleted_time < ?) OR " +
                "  (deleted_time IS NULL AND (last_seen IS NULL OR last_seen < ?))" +
                ")")) {
            ps.setString(1, ROOT_SENTINEL);
            ps.setString(2, cutoff);
            ps.setString(3, cutoff);
            ps.executeUpdate();
        }
    }

    private String parentPathId(String path) {
        int idx = path.lastIndexOf('/');
        if (idx <= 0) return ROOT_SENTINEL;
        return hashPath(path.substring(0, idx));
    }

    private void ensureParentChain(String path) throws SQLException {
        int idx = path.lastIndexOf('/');
        if (idx <= 0) return;
        String parentPath = path.substring(0, idx);
        ensureParentChain(parentPath);
        ensureDirectoryRow(parentPath);
    }

    private void ensureDirectoryRow(String dirPath) throws SQLException {
        String id = hashPath(dirPath);
        String parentId = parentPathId(dirPath);
        int slash = dirPath.lastIndexOf('/');
        String basename = (slash >= 0) ? dirPath.substring(slash + 1) : dirPath;
        try (PreparedStatement ps = conn.prepareStatement(
                "INSERT OR IGNORE INTO snapshot " +
                "(id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time) " +
                "VALUES (?, ?, ?, ?, ?, NULL, NULL)")) {
            ps.setString(1, id);
            ps.setString(2, parentId);
            ps.setString(3, basename);
            ps.setString(4, "1970-01-01_00-00-00_000000Z");
            ps.setLong(5, -1L);
            ps.executeUpdate();
        }
    }
}
