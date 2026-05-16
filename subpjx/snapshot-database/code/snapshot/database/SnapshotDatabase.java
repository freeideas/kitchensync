package snapshot.database;

import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Statement;
import java.util.Optional;

public final class SnapshotDatabase implements AutoCloseable {
    private static final String BASE62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    private static final long P1 = 0x9E3779B185EBCA87L;
    private static final long P2 = 0xC2B2AE3D27D4EB4FL;
    private static final long P3 = 0x165667B19E3779F9L;
    private static final long P4 = 0x85EBCA77C2B2AE63L;
    private static final long P5 = 0x27D4EB2F165667C5L;
    private static final String ROOT_PARENT_ID = pathIdUnchecked("/");

    private final Connection connection;
    private boolean closed;

    private SnapshotDatabase(Connection connection) {
        this.connection = connection;
    }

    public static SnapshotDatabase open(String db_file) {
        if (db_file == null) {
            throw new SnapshotDatabaseException("database_error", "database file is required");
        }
        return open(Path.of(db_file));
    }

    public static SnapshotDatabase open(Path db_file) {
        if (db_file == null) {
            throw new SnapshotDatabaseException("database_error", "database file is required");
        }
        try {
            Connection connection = DriverManager.getConnection("jdbc:sqlite:" + db_file.toAbsolutePath());
            try (Statement statement = connection.createStatement()) {
                statement.execute("PRAGMA foreign_keys = ON");
                try (ResultSet ignored = statement.executeQuery("PRAGMA journal_mode = DELETE")) {
                    ignored.next();
                }
                if (!schemaObjectExists(connection, "table", "snapshot")) {
                    statement.execute("""
                            CREATE TABLE snapshot (
                                id TEXT PRIMARY KEY,
                                parent_id TEXT NOT NULL,
                                basename TEXT NOT NULL,
                                mod_time TEXT NOT NULL,
                                byte_size INTEGER NOT NULL,
                                last_seen TEXT,
                                deleted_time TEXT
                            )
                            """);
                }
                if (!schemaObjectExists(connection, "index", "snapshot_parent_id_idx")) {
                    statement.execute("CREATE INDEX snapshot_parent_id_idx ON snapshot(parent_id)");
                }
                if (!schemaObjectExists(connection, "index", "snapshot_last_seen_idx")) {
                    statement.execute("CREATE INDEX snapshot_last_seen_idx ON snapshot(last_seen)");
                }
                if (!schemaObjectExists(connection, "index", "snapshot_deleted_time_idx")) {
                    statement.execute("CREATE INDEX snapshot_deleted_time_idx ON snapshot(deleted_time)");
                }
            }
            return new SnapshotDatabase(connection);
        } catch (SQLException e) {
            throw databaseError(e);
        }
    }

    @Override
    public void close() {
        if (closed) {
            return;
        }
        closed = true;
        try {
            connection.close();
        } catch (SQLException e) {
            throw databaseError(e);
        }
    }

    public boolean has_rows() {
        ensureOpen();
        try (Statement statement = connection.createStatement();
             ResultSet rows = statement.executeQuery("SELECT 1 FROM snapshot LIMIT 1")) {
            return rows.next();
        } catch (SQLException e) {
            throw databaseError(e);
        }
    }

    public static String root_parent_id() {
        return ROOT_PARENT_ID;
    }

    public static String path_id(String relative_path) {
        return pathIdUnchecked(normalize(relative_path));
    }

    public Optional<SnapshotRow> lookup(String relative_path) {
        ensureOpen();
        String normalized = normalize(relative_path);
        try (PreparedStatement statement = connection.prepareStatement(
                "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time FROM snapshot WHERE id = ?")) {
            statement.setString(1, pathIdUnchecked(normalized));
            try (ResultSet rows = statement.executeQuery()) {
                if (!rows.next()) {
                    return Optional.empty();
                }
                return Optional.of(row(rows, normalized));
            }
        } catch (SQLException e) {
            throw databaseError(e);
        }
    }

    public void record_present(String relative_path, EntryMetadata metadata, SnapshotTime seen_at) {
        requireMetadata(metadata);
        requireTime(seen_at);
        String normalized = normalize(relative_path);
        transaction(() -> {
            upsert(normalized, metadata, seen_at, true);
            return null;
        });
    }

    public void record_copy_pending(String relative_path, EntryMetadata metadata) {
        requireMetadata(metadata);
        String normalized = normalize(relative_path);
        transaction(() -> {
            try (PreparedStatement statement = connection.prepareStatement("""
                    INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
                    VALUES (?, ?, ?, ?, ?, NULL, NULL)
                    ON CONFLICT(id) DO UPDATE SET
                        parent_id = excluded.parent_id,
                        basename = excluded.basename,
                        mod_time = excluded.mod_time,
                        byte_size = excluded.byte_size,
                        deleted_time = NULL
                    """)) {
                bindEntry(statement, normalized, metadata);
                statement.executeUpdate();
            }
            return null;
        });
    }

    public void confirm_copy_completed(String relative_path, SnapshotTime seen_at) {
        requireTime(seen_at);
        String normalized = normalize(relative_path);
        transaction(() -> {
            try (PreparedStatement statement = connection.prepareStatement(
                    "UPDATE snapshot SET last_seen = ? WHERE id = ?")) {
                statement.setString(1, seen_at.value());
                statement.setString(2, pathIdUnchecked(normalized));
                if (statement.executeUpdate() == 0) {
                    throw new SnapshotDatabaseException("not_found", "row not found");
                }
            }
            return null;
        });
    }

    public void mark_absent(String relative_path) {
        String normalized = normalize(relative_path);
        transaction(() -> {
            try (PreparedStatement statement = connection.prepareStatement(
                    "UPDATE snapshot SET deleted_time = last_seen WHERE id = ? AND deleted_time IS NULL")) {
                statement.setString(1, pathIdUnchecked(normalized));
                statement.executeUpdate();
            }
            return null;
        });
    }

    public void mark_displaced(String relative_path) {
        String normalized = normalize(relative_path);
        transaction(() -> {
            String id = pathIdUnchecked(normalized);
            String deletedTime;
            try (PreparedStatement statement = connection.prepareStatement(
                    "SELECT last_seen, deleted_time FROM snapshot WHERE id = ?")) {
                statement.setString(1, id);
                try (ResultSet rows = statement.executeQuery()) {
                    if (!rows.next()) {
                        return null;
                    }
                    deletedTime = rows.getString("deleted_time");
                    if (deletedTime == null) {
                        deletedTime = rows.getString("last_seen");
                    }
                }
            }
            try (PreparedStatement statement = connection.prepareStatement("""
                    WITH RECURSIVE subtree(id) AS (
                        VALUES(?)
                        UNION ALL
                        SELECT s.id FROM snapshot s
                        JOIN subtree st ON s.parent_id = st.id
                        WHERE s.deleted_time IS NULL
                    )
                    UPDATE snapshot
                    SET deleted_time = ?
                    WHERE deleted_time IS NULL
                    AND id IN (SELECT id FROM subtree)
                    """)) {
                statement.setString(1, id);
                statement.setString(2, deletedTime);
                statement.executeUpdate();
            }
            return null;
        });
    }

    public PurgeResult purge(SnapshotTime cutoff_time) {
        requireTime(cutoff_time);
        return transaction(() -> {
            try (PreparedStatement statement = connection.prepareStatement("""
                    DELETE FROM snapshot
                    WHERE (deleted_time IS NOT NULL AND deleted_time < ?)
                       OR (deleted_time IS NULL AND last_seen IS NOT NULL AND last_seen < ?)
                       OR (deleted_time IS NULL AND last_seen IS NULL)
                    """)) {
                statement.setString(1, cutoff_time.value());
                statement.setString(2, cutoff_time.value());
                return new PurgeResult(statement.executeUpdate());
            }
        });
    }

    private void upsert(String normalized, EntryMetadata metadata, SnapshotTime seenAt, boolean updateLastSeen) throws SQLException {
        String lastSeenSql = updateLastSeen ? "excluded.last_seen" : "snapshot.last_seen";
        try (PreparedStatement statement = connection.prepareStatement("""
                INSERT INTO snapshot (id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time)
                VALUES (?, ?, ?, ?, ?, ?, NULL)
                ON CONFLICT(id) DO UPDATE SET
                    parent_id = excluded.parent_id,
                    basename = excluded.basename,
                    mod_time = excluded.mod_time,
                    byte_size = excluded.byte_size,
                    last_seen = %s,
                    deleted_time = NULL
                """.formatted(lastSeenSql))) {
            bindEntry(statement, normalized, metadata);
            statement.setString(6, seenAt == null ? null : seenAt.value());
            statement.executeUpdate();
        }
    }

    private void bindEntry(PreparedStatement statement, String normalized, EntryMetadata metadata) throws SQLException {
        statement.setString(1, pathIdUnchecked(normalized));
        statement.setString(2, parentId(normalized));
        statement.setString(3, basename(normalized));
        statement.setString(4, metadata.mod_time().value());
        statement.setLong(5, metadata.byte_size());
    }

    private SnapshotRow row(ResultSet rows, String relativePath) throws SQLException {
        long byteSize = rows.getLong("byte_size");
        return new SnapshotRow(
                rows.getString("id"),
                rows.getString("parent_id"),
                relativePath,
                rows.getString("basename"),
                byteSize == -1 ? EntryKind.DIRECTORY : EntryKind.FILE,
                new SnapshotTime(rows.getString("mod_time")),
                byteSize,
                Optional.ofNullable(rows.getString("last_seen")).map(SnapshotTime::new),
                Optional.ofNullable(rows.getString("deleted_time")).map(SnapshotTime::new));
    }

    private static String normalize(String relativePath) {
        if (relativePath == null
                || relativePath.isEmpty()
                || relativePath.equals("/")
                || relativePath.startsWith("/")
                || relativePath.endsWith("/")
                || relativePath.contains("//")
                || relativePath.indexOf('\0') >= 0) {
            throw new SnapshotDatabaseException("invalid_path", "invalid path");
        }
        return relativePath;
    }

    private static String parentId(String normalized) {
        int slash = normalized.lastIndexOf('/');
        if (slash < 0) {
            return ROOT_PARENT_ID;
        }
        return pathIdUnchecked(normalized.substring(0, slash));
    }

    private static String basename(String normalized) {
        int slash = normalized.lastIndexOf('/');
        return slash < 0 ? normalized : normalized.substring(slash + 1);
    }

    private static void requireMetadata(EntryMetadata metadata) {
        if (metadata == null) {
            throw new SnapshotDatabaseException("invalid_metadata", "metadata is required");
        }
    }

    private static void requireTime(SnapshotTime time) {
        if (time == null) {
            throw new SnapshotDatabaseException("invalid_timestamp", "timestamp is required");
        }
    }

    private void ensureOpen() {
        if (closed) {
            throw new SnapshotDatabaseException("database_error", "database is closed");
        }
    }

    private <T> T transaction(SqlWork<T> work) {
        ensureOpen();
        try {
            boolean previousAutoCommit = connection.getAutoCommit();
            connection.setAutoCommit(false);
            try {
                T result = work.run();
                connection.commit();
                return result;
            } catch (RuntimeException | SQLException e) {
                connection.rollback();
                throw e;
            } finally {
                connection.setAutoCommit(previousAutoCommit);
            }
        } catch (SnapshotDatabaseException e) {
            throw e;
        } catch (SQLException e) {
            throw databaseError(e);
        }
    }

    private static SnapshotDatabaseException databaseError(SQLException e) {
        return new SnapshotDatabaseException("database_error", "database error", e);
    }

    private static boolean schemaObjectExists(Connection connection, String type, String name) throws SQLException {
        try (PreparedStatement statement = connection.prepareStatement(
                "SELECT 1 FROM sqlite_master WHERE type = ? AND name = ?")) {
            statement.setString(1, type);
            statement.setString(2, name);
            try (ResultSet rows = statement.executeQuery()) {
                return rows.next();
            }
        }
    }

    private static String pathIdUnchecked(String path) {
        long hash = xxHash64(path.getBytes(StandardCharsets.UTF_8));
        byte[] bytes = new byte[8];
        for (int i = 7; i >= 0; i--) {
            bytes[i] = (byte) hash;
            hash >>>= 8;
        }
        BigInteger value = new BigInteger(1, bytes);
        StringBuilder out = new StringBuilder();
        BigInteger radix = BigInteger.valueOf(62);
        if (value.signum() == 0) {
            out.append('0');
        }
        while (value.signum() > 0) {
            BigInteger[] divided = value.divideAndRemainder(radix);
            out.append(BASE62.charAt(divided[1].intValue()));
            value = divided[0];
        }
        while (out.length() < 11) {
            out.append('0');
        }
        return out.reverse().toString();
    }

    private static long xxHash64(byte[] data) {
        int index = 0;
        int length = data.length;
        long hash;
        if (length >= 32) {
            long v1 = P1 + P2;
            long v2 = P2;
            long v3 = 0;
            long v4 = -P1;
            int limit = length - 32;
            while (index <= limit) {
                v1 = round(v1, readLong(data, index));
                index += 8;
                v2 = round(v2, readLong(data, index));
                index += 8;
                v3 = round(v3, readLong(data, index));
                index += 8;
                v4 = round(v4, readLong(data, index));
                index += 8;
            }
            hash = Long.rotateLeft(v1, 1)
                    + Long.rotateLeft(v2, 7)
                    + Long.rotateLeft(v3, 12)
                    + Long.rotateLeft(v4, 18);
            hash = mergeRound(hash, v1);
            hash = mergeRound(hash, v2);
            hash = mergeRound(hash, v3);
            hash = mergeRound(hash, v4);
        } else {
            hash = P5;
        }
        hash += length;
        while (index <= length - 8) {
            hash ^= round(0, readLong(data, index));
            hash = Long.rotateLeft(hash, 27) * P1 + P4;
            index += 8;
        }
        if (index <= length - 4) {
            hash ^= (readInt(data, index) & 0xFFFF_FFFFL) * P1;
            hash = Long.rotateLeft(hash, 23) * P2 + P3;
            index += 4;
        }
        while (index < length) {
            hash ^= (data[index] & 0xFFL) * P5;
            hash = Long.rotateLeft(hash, 11) * P1;
            index++;
        }
        hash ^= hash >>> 33;
        hash *= P2;
        hash ^= hash >>> 29;
        hash *= P3;
        hash ^= hash >>> 32;
        return hash;
    }

    private static long round(long accumulator, long input) {
        accumulator += input * P2;
        accumulator = Long.rotateLeft(accumulator, 31);
        accumulator *= P1;
        return accumulator;
    }

    private static long mergeRound(long hash, long value) {
        hash ^= round(0, value);
        return hash * P1 + P4;
    }

    private static long readLong(byte[] data, int index) {
        return (data[index] & 0xFFL)
                | ((data[index + 1] & 0xFFL) << 8)
                | ((data[index + 2] & 0xFFL) << 16)
                | ((data[index + 3] & 0xFFL) << 24)
                | ((data[index + 4] & 0xFFL) << 32)
                | ((data[index + 5] & 0xFFL) << 40)
                | ((data[index + 6] & 0xFFL) << 48)
                | ((data[index + 7] & 0xFFL) << 56);
    }

    private static int readInt(byte[] data, int index) {
        return (data[index] & 0xFF)
                | ((data[index + 1] & 0xFF) << 8)
                | ((data[index + 2] & 0xFF) << 16)
                | ((data[index + 3] & 0xFF) << 24);
    }

    @FunctionalInterface
    private interface SqlWork<T> {
        T run() throws SQLException;
    }
}
