package snapshot.db;

import java.sql.*;
import java.util.*;

public final class SnapshotStore implements AutoCloseable {

    private final Connection conn;

    private SnapshotStore(Connection conn) {
        this.conn = conn;
    }

    public static SnapshotStore open(String file) throws SQLException {
        try { Class.forName("org.sqlite.JDBC"); } catch (ClassNotFoundException ignored) {}
        Connection conn = DriverManager.getConnection("jdbc:sqlite:" + file);
        conn.setAutoCommit(true);
        try (Statement st = conn.createStatement()) {
            st.executeUpdate("PRAGMA journal_mode=WAL");
            st.executeUpdate("""
                CREATE TABLE IF NOT EXISTS snapshot (
                    id           TEXT NOT NULL PRIMARY KEY,
                    parent_id    TEXT NOT NULL,
                    basename     TEXT NOT NULL,
                    mod_time     TEXT NOT NULL,
                    byte_size    INTEGER NOT NULL,
                    last_seen    TEXT,
                    deleted_time TEXT
                )""");
            st.executeUpdate(
                "CREATE INDEX IF NOT EXISTS idx_parent   ON snapshot(parent_id)");
            st.executeUpdate(
                "CREATE INDEX IF NOT EXISTS idx_last     ON snapshot(last_seen)");
            st.executeUpdate(
                "CREATE INDEX IF NOT EXISTS idx_deleted  ON snapshot(deleted_time)");
        }
        return new SnapshotStore(conn);
    }

    @Override
    public void close() throws SQLException {
        conn.close();
    }

    public void upsertObserved(String path, String modTime, long byteSize,
                               boolean isDir, String now) throws SQLException {
        String id = PathIdentity.identify(path);
        String parentId = parentIdOf(path);
        String basename = basenameOf(path);
        long size = isDir ? -1L : byteSize;
        try (PreparedStatement ps = conn.prepareStatement("""
                INSERT INTO snapshot(id,parent_id,basename,mod_time,byte_size,last_seen,deleted_time)
                VALUES(?,?,?,?,?,?,NULL)
                ON CONFLICT(id) DO UPDATE SET
                    mod_time=excluded.mod_time,
                    byte_size=excluded.byte_size,
                    last_seen=excluded.last_seen,
                    deleted_time=NULL""")) {
            ps.setString(1, id);
            ps.setString(2, parentId);
            ps.setString(3, basename);
            ps.setString(4, modTime);
            ps.setLong(5, size);
            ps.setString(6, now);
            ps.executeUpdate();
        }
    }

    public void recordDecided(String path, String modTime, long byteSize,
                              boolean isDir) throws SQLException {
        String id = PathIdentity.identify(path);
        String parentId = parentIdOf(path);
        String basename = basenameOf(path);
        long size = isDir ? -1L : byteSize;
        try (PreparedStatement ps = conn.prepareStatement("""
                INSERT INTO snapshot(id,parent_id,basename,mod_time,byte_size,last_seen,deleted_time)
                VALUES(?,?,?,?,?,NULL,NULL)
                ON CONFLICT(id) DO UPDATE SET
                    mod_time=excluded.mod_time,
                    byte_size=excluded.byte_size,
                    deleted_time=NULL""")) {
            ps.setString(1, id);
            ps.setString(2, parentId);
            ps.setString(3, basename);
            ps.setString(4, modTime);
            ps.setLong(5, size);
            ps.executeUpdate();
        }
    }

    public void confirmPresent(String path, String now) throws SQLException {
        String id = PathIdentity.identify(path);
        try (PreparedStatement ps = conn.prepareStatement(
                "UPDATE snapshot SET last_seen=? WHERE id=?")) {
            ps.setString(1, now);
            ps.setString(2, id);
            ps.executeUpdate();
        }
    }

    public void markSubtreeDeleted(String path, String deletedTime) throws SQLException {
        String id = PathIdentity.identify(path);
        try (PreparedStatement ps = conn.prepareStatement("""
                WITH RECURSIVE sub(id) AS (
                    SELECT id FROM snapshot WHERE id=?
                    UNION ALL
                    SELECT s.id FROM snapshot s JOIN sub ON s.parent_id=sub.id
                )
                UPDATE snapshot SET deleted_time=?
                WHERE id IN (SELECT id FROM sub) AND deleted_time IS NULL""")) {
            ps.setString(1, id);
            ps.setString(2, deletedTime);
            ps.executeUpdate();
        }
    }

    public Optional<SnapshotRecord> lookup(String path) throws SQLException {
        String id = PathIdentity.identify(path);
        try (PreparedStatement ps = conn.prepareStatement(
                "SELECT id,parent_id,basename,mod_time,byte_size,last_seen,deleted_time "
                + "FROM snapshot WHERE id=?")) {
            ps.setString(1, id);
            try (ResultSet rs = ps.executeQuery()) {
                if (!rs.next()) return Optional.empty();
                return Optional.of(toRecord(rs));
            }
        }
    }

    public List<SnapshotRecord> listChildren(String parentPath) throws SQLException {
        String parentId = PathIdentity.identify(parentPath);
        try (PreparedStatement ps = conn.prepareStatement(
                "SELECT id,parent_id,basename,mod_time,byte_size,last_seen,deleted_time "
                + "FROM snapshot WHERE parent_id=?")) {
            ps.setString(1, parentId);
            List<SnapshotRecord> result = new ArrayList<>();
            try (ResultSet rs = ps.executeQuery()) {
                while (rs.next()) result.add(toRecord(rs));
            }
            return result;
        }
    }

    public void purgeOlderThan(int retentionDays, String now) throws SQLException {
        String cutoff = Timestamps.subtractDays(now, retentionDays);
        try (PreparedStatement ps = conn.prepareStatement("""
                DELETE FROM snapshot WHERE
                    (deleted_time IS NOT NULL AND deleted_time < ?)
                    OR (deleted_time IS NULL AND (last_seen IS NULL OR last_seen < ?))""")) {
            ps.setString(1, cutoff);
            ps.setString(2, cutoff);
            ps.executeUpdate();
        }
    }

    private static SnapshotRecord toRecord(ResultSet rs) throws SQLException {
        return new SnapshotRecord(
                rs.getString("id"),
                rs.getString("parent_id"),
                rs.getString("basename"),
                rs.getString("mod_time"),
                rs.getLong("byte_size"),
                rs.getString("last_seen"),
                rs.getString("deleted_time"));
    }

    private static String parentIdOf(String path) {
        int slash = path.lastIndexOf('/');
        return PathIdentity.identify(slash <= 0 ? "" : path.substring(0, slash));
    }

    private static String basenameOf(String path) {
        int slash = path.lastIndexOf('/');
        return slash < 0 ? path : path.substring(slash + 1);
    }
}
