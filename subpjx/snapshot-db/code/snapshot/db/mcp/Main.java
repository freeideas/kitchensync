package snapshot.db.mcp;

import snapshot.db.*;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.sql.SQLException;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

public final class Main {

    private static final String RECORD_SCHEMA =
        "{\"additionalProperties\":false,"
        + "\"properties\":{"
          + "\"basename\":{\"type\":\"string\"},"
          + "\"byte_size\":{\"type\":\"integer\"},"
          + "\"deleted_time\":{\"type\":[\"null\",\"string\"]},"
          + "\"id\":{\"type\":\"string\"},"
          + "\"last_seen\":{\"type\":[\"null\",\"string\"]},"
          + "\"mod_time\":{\"type\":\"string\"},"
          + "\"parent_id\":{\"type\":\"string\"}"
        + "},"
        + "\"required\":[\"basename\",\"byte_size\",\"deleted_time\",\"id\",\"last_seen\",\"mod_time\",\"parent_id\"],"
        + "\"type\":\"object\"}";

    private static final String EMPTY_OUT =
        "{\"additionalProperties\":false,\"properties\":{},\"type\":\"object\"}";

    private static final String OPEN_OUT =
        "{\"additionalProperties\":false,\"properties\":{\"handle\":{\"type\":\"string\"}},"
        + "\"required\":[\"handle\"],\"type\":\"object\"}";

    private static final String TOOLS_LIST_JSON =
        "{\"tools\":["
        + "{\"description\":\"Close the snapshot store for the given handle.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"handle\":{\"type\":\"string\"}},\"required\":[\"handle\"],\"type\":\"object\"},"
          + "\"name\":\"close\","
          + "\"outputSchema\":" + EMPTY_OUT + "},"
        + "{\"description\":\"Set last_seen to now on the row at path; no-op if no row exists.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"handle\":{\"type\":\"string\"},\"now\":{\"type\":\"string\"},\"path\":{\"type\":\"string\"}},\"required\":[\"handle\",\"now\",\"path\"],\"type\":\"object\"},"
          + "\"name\":\"confirm-present\","
          + "\"outputSchema\":" + EMPTY_OUT + "},"
        + "{\"description\":\"Compute the 11-character base62 path identity for a relative path.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"path\":{\"type\":\"string\"}},\"required\":[\"path\"],\"type\":\"object\"},"
          + "\"name\":\"identify\","
          + "\"outputSchema\":{\"type\":\"string\"}},"
        + "{\"description\":\"Return every row whose parent_id equals the identity of parent_path.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"handle\":{\"type\":\"string\"},\"parent_path\":{\"type\":\"string\"}},\"required\":[\"handle\",\"parent_path\"],\"type\":\"object\"},"
          + "\"name\":\"list-children\","
          + "\"outputSchema\":{\"additionalProperties\":false,\"properties\":{\"records\":{\"items\":" + RECORD_SCHEMA + ",\"type\":\"array\"}},\"required\":[\"records\"],\"type\":\"object\"}},"
        + "{\"description\":\"Return the row stored for path, or null if no row exists.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"handle\":{\"type\":\"string\"},\"path\":{\"type\":\"string\"}},\"required\":[\"handle\",\"path\"],\"type\":\"object\"},"
          + "\"name\":\"lookup\","
          + "\"outputSchema\":{\"additionalProperties\":false,\"properties\":{\"record\":{\"oneOf\":[{\"type\":\"null\"}," + RECORD_SCHEMA + "]}},\"required\":[\"record\"],\"type\":\"object\"}},"
        + "{\"description\":\"Set deleted_time on the row at path and all transitive descendants with null deleted_time.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"deleted_time\":{\"type\":\"string\"},\"handle\":{\"type\":\"string\"},\"path\":{\"type\":\"string\"}},\"required\":[\"deleted_time\",\"handle\",\"path\"],\"type\":\"object\"},"
          + "\"name\":\"mark-subtree-deleted\","
          + "\"outputSchema\":" + EMPTY_OUT + "},"
        + "{\"description\":\"Return the current UTC time as a process-monotonic timestamp string.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{},\"type\":\"object\"},"
          + "\"name\":\"now\","
          + "\"outputSchema\":{\"additionalProperties\":false,\"properties\":{\"timestamp\":{\"type\":\"string\"}},\"required\":[\"timestamp\"],\"type\":\"object\"}},"
        + "{\"description\":\"Return count process-monotonic UTC timestamp strings.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"count\":{\"type\":\"integer\"}},\"required\":[\"count\"],\"type\":\"object\"},"
          + "\"name\":\"now-n\","
          + "\"outputSchema\":{\"type\":\"array\",\"items\":{\"type\":\"string\"}}},"
        + "{\"description\":\"Open or create the snapshot database at the given file path.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"file\":{\"type\":\"string\"}},\"required\":[\"file\"],\"type\":\"object\"},"
          + "\"name\":\"open\","
          + "\"outputSchema\":" + OPEN_OUT + "},"
        + "{\"description\":\"Delete tombstoned rows and non-tombstone rows older than retention_days or with null last_seen.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"handle\":{\"type\":\"string\"},\"now\":{\"type\":\"string\"},\"retention_days\":{\"type\":\"integer\"}},\"required\":[\"handle\",\"now\",\"retention_days\"],\"type\":\"object\"},"
          + "\"name\":\"purge-older-than\","
          + "\"outputSchema\":" + EMPTY_OUT + "},"
        + "{\"description\":\"Record a decision about path; last_seen left null on insert and unchanged on update.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"byte_size\":{\"type\":\"integer\"},\"handle\":{\"type\":\"string\"},\"is_dir\":{\"type\":\"boolean\"},\"mod_time\":{\"type\":\"string\"},\"path\":{\"type\":\"string\"}},\"required\":[\"byte_size\",\"handle\",\"is_dir\",\"mod_time\",\"path\"],\"type\":\"object\"},"
          + "\"name\":\"record-decided\","
          + "\"outputSchema\":" + EMPTY_OUT + "},"
        + "{\"description\":\"Record that path was directly observed present, inserting or updating its row.\","
          + "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"byte_size\":{\"type\":\"integer\"},\"handle\":{\"type\":\"string\"},\"is_dir\":{\"type\":\"boolean\"},\"mod_time\":{\"type\":\"string\"},\"now\":{\"type\":\"string\"},\"path\":{\"type\":\"string\"}},\"required\":[\"byte_size\",\"handle\",\"is_dir\",\"mod_time\",\"now\",\"path\"],\"type\":\"object\"},"
          + "\"name\":\"upsert-observed\","
          + "\"outputSchema\":" + EMPTY_OUT + "}"
        + "]}";

    private static final Map<String, SnapshotStore> stores = new ConcurrentHashMap<>();

    public static void main(String[] args) throws IOException {
        ServerSocket server = new ServerSocket(0, 50, InetAddress.getLoopbackAddress());
        System.out.println("MCP_PORT=" + server.getLocalPort());
        System.out.flush();
        //noinspection InfiniteLoopStatement
        while (true) {
            Socket conn = server.accept();
            new Thread(() -> handleConnection(conn)).start();
        }
    }

    private static void handleConnection(Socket conn) {
        try (conn;
             BufferedReader in = new BufferedReader(
                     new InputStreamReader(conn.getInputStream(), StandardCharsets.UTF_8));
             PrintWriter out = new PrintWriter(
                     new OutputStreamWriter(conn.getOutputStream(), StandardCharsets.UTF_8), true)) {
            String line;
            while ((line = in.readLine()) != null) {
                if (line.isBlank()) continue;
                String response = handleLine(line);
                if (response != null) out.println(response);
            }
        } catch (IOException ignored) {}
    }

    private static String handleLine(String line) {
        Object id = null;
        try {
            Object parsed = Json.parse(line);
            if (!(parsed instanceof Map)) return errorResponse(null, -32600, "invalid request");
            @SuppressWarnings("unchecked") Map<String, Object> req = (Map<String, Object>) parsed;

            id = req.get("id");
            if (id == null) return null;

            Object methodObj = req.get("method");
            if (!(methodObj instanceof String)) return errorResponse(id, -32600, "invalid request");
            String method = (String) methodObj;

            if ("tools/list".equals(method)) {
                return "{\"id\":" + Json.write(id) + ",\"jsonrpc\":\"2.0\",\"result\":" + TOOLS_LIST_JSON + "}";
            }

            if ("tools/call".equals(method)) {
                Object paramsObj = req.get("params");
                if (!(paramsObj instanceof Map))
                    return errorResponse(id, -32602, "invalid params");
                @SuppressWarnings("unchecked") Map<String, Object> params =
                        (Map<String, Object>) paramsObj;
                Object nameObj = params.get("name");
                if (!(nameObj instanceof String))
                    return errorResponse(id, -32602, "invalid params");
                String name = (String) nameObj;
                Object argsObj = params.get("arguments");
                if (!(argsObj instanceof Map))
                    return errorResponse(id, -32602, "invalid params");
                @SuppressWarnings("unchecked") Map<String, Object> arguments =
                        (Map<String, Object>) argsObj;
                return dispatchTool(id, name, arguments);
            }

            return errorResponse(id, -32601, "method not found: " + method);

        } catch (Json.JsonException e) {
            return errorResponse(id, -32700, "parse error: " + e.getMessage());
        } catch (Exception e) {
            return errorResponse(id, -32603, "internal error: " + e.getMessage());
        }
    }

    private static String dispatchTool(Object id, String name, Map<String, Object> args) {
        try {
            String normalizedName = name.replace('_', '-');
            return switch (normalizedName) {
                case "close"                -> toolClose(id, args);
                case "confirm-present"      -> toolConfirmPresent(id, args);
                case "identify"             -> toolIdentify(id, args);
                case "list-children"        -> toolListChildren(id, args);
                case "lookup"               -> toolLookup(id, args);
                case "mark-subtree-deleted" -> toolMarkSubtreeDeleted(id, args);
                case "now"                  -> toolNow(id);
                case "now-n"                -> toolNowN(id, args);
                case "open"                 -> toolOpen(id, args);
                case "purge-older-than"     -> toolPurgeOlderThan(id, args);
                case "record-decided"       -> toolRecordDecided(id, args);
                case "upsert-observed"      -> toolUpsertObserved(id, args);
                default                     -> errorResponse(id, -32000, "not implemented");
            };
        } catch (IllegalArgumentException e) {
            return errorResponse(id, -32000, "invalid argument: " + e.getMessage());
        } catch (SQLException e) {
            return errorResponse(id, -32000, e.getMessage());
        } catch (RuntimeException e) {
            if (e.getCause() instanceof SQLException sq)
                return errorResponse(id, -32000, sq.getMessage());
            return errorResponse(id, -32603, "internal error: " + e.getMessage());
        } catch (Exception e) {
            return errorResponse(id, -32603, "internal error: " + e.getMessage());
        }
    }

    private static String toolOpen(Object id, Map<String, Object> args) throws SQLException {
        String file = requireString(args, "file");
        String handle = UUID.randomUUID().toString();
        stores.put(handle, SnapshotStore.open(file));
        Map<String, Object> fields = new LinkedHashMap<>();
        fields.put("handle", handle);
        return successResponse(id, fields);
    }

    private static String toolClose(Object id, Map<String, Object> args) throws SQLException {
        String handle = requireString(args, "handle");
        SnapshotStore store = stores.remove(handle);
        if (store != null) store.close();
        return successResponse(id, new LinkedHashMap<>());
    }

    private static String toolIdentify(Object id, Map<String, Object> args) {
        String path = requireString(args, "path");
        return successResponse(id, PathIdentity.identify(path));
    }

    private static String toolNow(Object id) {
        return successResponse(id, Timestamps.now());
    }

    private static String toolNowN(Object id, Map<String, Object> args) {
        long count = requireLong(args, "count");
        List<String> stamps = new ArrayList<>((int) count);
        for (int i = 0; i < count; i++) stamps.add(Timestamps.now());
        return successResponse(id, stamps);
    }

    private static String toolUpsertObserved(Object id, Map<String, Object> args) throws SQLException {
        SnapshotStore store = requireStore(args);
        String path    = requireString(args, "path");
        String modTime = requireString(args, "mod_time");
        long byteSize  = requireLong(args, "byte_size");
        boolean isDir  = requireBoolean(args, "is_dir");
        String now     = requireString(args, "now");
        store.upsertObserved(path, modTime, byteSize, isDir, now);
        return successResponse(id, new LinkedHashMap<>());
    }

    private static String toolRecordDecided(Object id, Map<String, Object> args) throws SQLException {
        SnapshotStore store = requireStore(args);
        String path    = requireString(args, "path");
        String modTime = requireString(args, "mod_time");
        long byteSize  = requireLong(args, "byte_size");
        boolean isDir  = requireBoolean(args, "is_dir");
        store.recordDecided(path, modTime, byteSize, isDir);
        return successResponse(id, new LinkedHashMap<>());
    }

    private static String toolConfirmPresent(Object id, Map<String, Object> args) throws SQLException {
        SnapshotStore store = requireStore(args);
        String path = requireString(args, "path");
        String now  = requireString(args, "now");
        store.confirmPresent(path, now);
        return successResponse(id, new LinkedHashMap<>());
    }

    private static String toolMarkSubtreeDeleted(Object id, Map<String, Object> args) throws SQLException {
        SnapshotStore store = requireStore(args);
        String path        = requireString(args, "path");
        String deletedTime = requireString(args, "deleted_time");
        store.markSubtreeDeleted(path, deletedTime);
        return successResponse(id, new LinkedHashMap<>());
    }

    private static String toolLookup(Object id, Map<String, Object> args) throws SQLException {
        SnapshotStore store = requireStore(args);
        String path = requireString(args, "path");
        Optional<SnapshotRecord> rec = store.lookup(path);
        Map<String, Object> fields = new LinkedHashMap<>();
        fields.put("record", rec.map(Main::serializeRecord).orElse(null));
        return successResponse(id, fields);
    }

    private static String toolListChildren(Object id, Map<String, Object> args) throws SQLException {
        SnapshotStore store = requireStore(args);
        String parentPath = requireString(args, "parent_path");
        List<SnapshotRecord> children = store.listChildren(parentPath);
        List<Object> records = new ArrayList<>();
        for (SnapshotRecord r : children) records.add(serializeRecord(r));
        Map<String, Object> fields = new LinkedHashMap<>();
        fields.put("records", records);
        return successResponse(id, fields);
    }

    private static String toolPurgeOlderThan(Object id, Map<String, Object> args) throws SQLException {
        SnapshotStore store = requireStore(args);
        long retentionDays = requireLong(args, "retention_days");
        String now         = requireString(args, "now");
        store.purgeOlderThan((int) retentionDays, now);
        return successResponse(id, new LinkedHashMap<>());
    }

    private static Map<String, Object> serializeRecord(SnapshotRecord r) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("basename",     r.basename());
        m.put("byte_size",    r.byteSize());
        m.put("deleted_time", r.deletedTime());
        m.put("id",           r.id());
        m.put("last_seen",    r.lastSeen());
        m.put("mod_time",     r.modTime());
        m.put("parent_id",    r.parentId());
        return m;
    }

    private static SnapshotStore requireStore(Map<String, Object> args) {
        String handle = requireString(args, "handle");
        SnapshotStore store = stores.get(handle);
        if (store == null)
            throw new IllegalArgumentException("no open store for handle: " + handle);
        return store;
    }

    private static String requireString(Map<String, Object> args, String key) {
        Object v = args.get(key);
        if (!(v instanceof String))
            throw new IllegalArgumentException(key + " is required and must be a string");
        return (String) v;
    }

    private static long requireLong(Map<String, Object> args, String key) {
        Object v = args.get(key);
        if (!(v instanceof Number))
            throw new IllegalArgumentException(key + " is required and must be a number");
        return ((Number) v).longValue();
    }

    private static boolean requireBoolean(Map<String, Object> args, String key) {
        Object v = args.get(key);
        if (!(v instanceof Boolean))
            throw new IllegalArgumentException(key + " is required and must be a boolean");
        return (Boolean) v;
    }

    private static String successResponse(Object id, Object result) {
        return "{\"id\":" + Json.write(id)
                + ",\"jsonrpc\":\"2.0\",\"result\":" + Json.write(result) + "}";
    }

    private static String errorResponse(Object id, int code, String message) {
        String idPart = (id != null) ? Json.write(id) : "null";
        return "{\"error\":{\"code\":" + code + ",\"message\":"
                + Json.write(message) + "},\"id\":" + idPart + ",\"jsonrpc\":\"2.0\"}";
    }
}
