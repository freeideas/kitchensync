package snapshot.database.mcp;

import snapshot.database.EntryKind;
import snapshot.database.EntryMetadata;
import snapshot.database.PurgeResult;
import snapshot.database.SnapshotDatabase;
import snapshot.database.SnapshotDatabaseException;
import snapshot.database.SnapshotRow;
import snapshot.database.SnapshotTime;
import snapshot.database.SnapshotTimestampGenerator;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class Main {
    private static final Map<String, SnapshotDatabase> DATABASES = new ConcurrentHashMap<>();
    private static final SnapshotTimestampGenerator GENERATOR = new SnapshotTimestampGenerator();
    private static volatile boolean stopping;
    private static ServerSocket serverSocket;

    private Main() {
    }

    public static void main(String[] args) throws IOException {
        serverSocket = new ServerSocket(0, 50, InetAddress.getByName("127.0.0.1"));
        System.out.println("MCP_PORT=" + serverSocket.getLocalPort());
        System.out.flush();

        ExecutorService executor = Executors.newCachedThreadPool();
        while (!stopping) {
            try {
                Socket socket = serverSocket.accept();
                executor.submit(() -> serve(socket));
            } catch (IOException e) {
                if (!stopping) {
                    throw e;
                }
            }
        }
        executor.shutdownNow();
    }

    private static void serve(Socket socket) {
        try (socket;
             BufferedReader reader = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
             BufferedWriter writer = new BufferedWriter(new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null && !stopping) {
                Response response = dispatch(line);
                if (response.body() != null) {
                    writer.write(Json.stringify(response.body()));
                    writer.write('\n');
                    writer.flush();
                }
                if (response.shutdown()) {
                    shutdown();
                    return;
                }
            }
        } catch (IOException ignored) {
        }
    }

    private static Response dispatch(String line) {
        Object id = null;
        try {
            Object parsed = Json.parse(line);
            if (!(parsed instanceof Map<?, ?> request)) {
                return response(null, error(-32600, "invalid request"));
            }
            id = request.get("id");
            if (!request.containsKey("id")) {
                return "aitc/shutdown".equals(request.get("method")) ? new Response(null, true) : new Response(null, false);
            }
            if (!"2.0".equals(request.get("jsonrpc")) || !(request.get("method") instanceof String method)) {
                return response(id, error(-32600, "invalid request"));
            }
            return switch (method) {
                case "tools/list" -> response(id, result(Map.of("tools", tools())));
                case "tools/call" -> call(id, request.get("params"));
                case "aitc/shutdown" -> shutdownResponse(id, request.get("params"));
                default -> response(id, error(-32601, "method not found: " + method));
            };
        } catch (IllegalArgumentException e) {
            return response(id, error(-32700, "parse error"));
        } catch (InvalidParamsException e) {
            return response(id, error(-32602, "invalid params"));
        } catch (ToolException e) {
            return response(id, error(-32000, e.getMessage()));
        } catch (SnapshotDatabaseException e) {
            return response(id, error(-32000, e.category()));
        } catch (RuntimeException e) {
            return response(id, error(-32603, "internal error"));
        }
    }

    private static Response call(Object id, Object paramsValue) {
        Map<?, ?> params = requireParamsObject(paramsValue);
        String name = requireParamsString(params.get("name"));
        Map<?, ?> args = requireParamsArguments(params.get("arguments"));
        Object output = switch (name) {
            case "close" -> {
                validateArgs(args, "db_path");
                String path = string(args, "db_path");
                SnapshotDatabase db = DATABASES.remove(path);
                if (db != null) {
                    db.close();
                }
                yield Map.of();
            }
            case "confirm-copy-completed" -> {
                validateArgs(args, "db_path", "relative_path", "seen_at");
                database(args).confirm_copy_completed(string(args, "relative_path"), time(args, "seen_at"));
                yield Map.of();
            }
            case "generate-timestamp" -> {
                validateArgs(args);
                yield Map.of("timestamp", GENERATOR.next().value());
            }
            case "has-rows" -> {
                validateArgs(args, "db_path");
                yield Map.of("has_rows", database(args).has_rows());
            }
            case "lookup" -> {
                validateArgs(args, "db_path", "relative_path");
                Optional<SnapshotRow> found = database(args).lookup(string(args, "relative_path"));
                yield found.map(Main::rowMap).orElse(Map.of());
            }
            case "mark-absent" -> {
                validateArgs(args, "db_path", "relative_path");
                database(args).mark_absent(string(args, "relative_path"));
                yield Map.of();
            }
            case "mark-displaced" -> {
                validateArgs(args, "db_path", "relative_path");
                database(args).mark_displaced(string(args, "relative_path"));
                yield Map.of();
            }
            case "open" -> {
                validateArgs(args, "db_path");
                String path = string(args, "db_path");
                DATABASES.computeIfAbsent(path, SnapshotDatabase::open);
                yield Map.of();
            }
            case "path-id" -> {
                validateArgs(args, "relative_path");
                yield Map.of("id", SnapshotDatabase.path_id(string(args, "relative_path")));
            }
            case "purge" -> {
                validateArgs(args, "db_path", "cutoff_time");
                PurgeResult r = database(args).purge(time(args, "cutoff_time"));
                yield Map.of("deleted_count", r.deleted_rows());
            }
            case "record-copy-pending" -> {
                validateArgs(args, "db_path", "relative_path", "kind", "mod_time", "byte_size");
                database(args).record_copy_pending(string(args, "relative_path"), metadata(args));
                yield Map.of();
            }
            case "record-present" -> {
                validateArgs(args, "db_path", "relative_path", "kind", "mod_time", "byte_size", "seen_at");
                database(args).record_present(string(args, "relative_path"), metadata(args), time(args, "seen_at"));
                yield Map.of();
            }
            case "root-parent-id" -> {
                validateArgs(args);
                yield Map.of("id", SnapshotDatabase.root_parent_id());
            }
            default -> throw new ToolException("not implemented");
        };
        return response(id, result(output));
    }

    private static Response shutdownResponse(Object id, Object params) {
        if (params != null && !(params instanceof Map<?, ?> map && map.isEmpty())) {
            return response(id, error(-32602, "invalid params"));
        }
        return new Response(ok(id, Map.of()), true);
    }

    private static void shutdown() {
        stopping = true;
        for (SnapshotDatabase db : DATABASES.values()) {
            db.close();
        }
        try {
            serverSocket.close();
        } catch (IOException ignored) {
        }
        System.exit(0);
    }

    private static SnapshotDatabase database(Map<?, ?> args) {
        String path = string(args, "db_path");
        SnapshotDatabase db = DATABASES.get(path);
        if (db == null) {
            throw new SnapshotDatabaseException("database_error", "database not found: " + path);
        }
        return db;
    }

    private static EntryMetadata metadata(Map<?, ?> args) {
        requireMetadataField(args, "kind");
        requireMetadataField(args, "mod_time");
        requireMetadataField(args, "byte_size");
        return new EntryMetadata(
                EntryKind.fromWireName(string(args, "kind")),
                time(args, "mod_time"),
                longArg(args, "byte_size"));
    }

    private static void requireMetadataField(Map<?, ?> args, String key) {
        if (!args.containsKey(key)) {
            throw new SnapshotDatabaseException("invalid_metadata", "metadata is required");
        }
    }

    private static Map<String, Object> rowMap(SnapshotRow row) {
        return map(
                "basename", row.basename(),
                "byte_size", row.byte_size(),
                "deleted_time", row.deleted_time().map(SnapshotTime::value).orElse(null),
                "id", row.id(),
                "kind", row.kind().wireName(),
                "last_seen", row.last_seen().map(SnapshotTime::value).orElse(null),
                "mod_time", row.mod_time().value(),
                "parent_id", row.parent_id(),
                "relative_path", row.relative_path());
    }

    private static List<Map<String, Object>> tools() {
        return List.of(
                tool("close", "Close an open snapshot database.", dbPathSchema(), emptySchema()),
                tool("confirm-copy-completed", "Confirm a pending file copy completed.", dbPathWith("relative_path", "seen_at"), emptySchema()),
                tool("generate-timestamp", "Generate a strictly increasing snapshot timestamp.", emptySchema(), objectSchema(map("timestamp", stringSchema()), List.of("timestamp"))),
                tool("has-rows", "Return true when the snapshot table has rows.", dbPathSchema(), objectSchema(map("has_rows", map("type", "boolean")), List.of("has_rows"))),
                tool("lookup", "Look up one snapshot row by relative path.", dbPathWith("relative_path"), lookupOutputSchema()),
                tool("mark-absent", "Mark one path absent.", dbPathWith("relative_path"), emptySchema()),
                tool("mark-displaced", "Mark one path and its descendants displaced.", dbPathWith("relative_path"), emptySchema()),
                tool("open", "Open and initialize a snapshot database file.", dbPathSchema(), emptySchema()),
                tool("path-id", "Return the deterministic path ID for a relative path.", pathOnlySchema(), objectSchema(map("id", stringSchema()), List.of("id"))),
                tool("purge", "Delete stale rows older than a cutoff timestamp.", dbPathWith("cutoff_time"), objectSchema(map("deleted_count", map("type", "integer")), List.of("deleted_count"))),
                tool("record-copy-pending", "Record metadata for a decided file copy.", dbPathWith("relative_path", "kind", "mod_time", "byte_size"), emptySchema()),
                tool("record-present", "Record an entry confirmed present.", dbPathWith("relative_path", "kind", "mod_time", "byte_size", "seen_at"), emptySchema()),
                tool("root-parent-id", "Return the root-child parent sentinel path ID.", emptySchema(), objectSchema(map("id", stringSchema()), List.of("id"))));
    }

    private static Map<String, Object> tool(
            String name,
            String description,
            Map<String, Object> input,
            Map<String, Object> output) {
        return map("description", description, "inputSchema", input, "name", name, "outputSchema", output);
    }

    private static Map<String, Object> dbPathSchema() {
        return objectSchema(map("db_path", stringSchema()), List.of("db_path"));
    }

    private static Map<String, Object> dbPathWith(String... extras) {
        Map<String, Object> props = new TreeMap<>();
        props.put("db_path", stringSchema());
        List<String> required = new java.util.ArrayList<>();
        required.add("db_path");
        for (String extra : extras) {
            if (extra.equals("byte_size")) {
                props.put(extra, map("type", "integer"));
            } else {
                props.put(extra, stringSchema());
            }
            required.add(extra);
        }
        return objectSchema(props, required);
    }

    private static Map<String, Object> pathOnlySchema() {
        return objectSchema(map("relative_path", stringSchema()), List.of("relative_path"));
    }

    private static Map<String, Object> lookupOutputSchema() {
        Map<String, Object> props = new TreeMap<>();
        props.put("basename", stringSchema());
        props.put("byte_size", map("type", "integer"));
        props.put("deleted_time", map("type", List.of("string", "null")));
        props.put("id", stringSchema());
        props.put("kind", map("enum", List.of("file", "directory"), "type", "string"));
        props.put("last_seen", map("type", List.of("string", "null")));
        props.put("mod_time", stringSchema());
        props.put("parent_id", stringSchema());
        props.put("relative_path", stringSchema());
        return objectSchema(props, List.of());
    }

    private static Map<String, Object> emptySchema() {
        return objectSchema(Map.of(), List.of());
    }

    private static Map<String, Object> objectSchema(Map<String, Object> properties, List<String> required) {
        return map("additionalProperties", false, "properties", properties, "required", required, "type", "object");
    }

    private static Map<String, Object> stringSchema() {
        return map("type", "string");
    }

    private static SnapshotTime time(Map<?, ?> args, String key) {
        return new SnapshotTime(string(args, key));
    }

    private static String string(Map<?, ?> map, String key) {
        Object value = map.get(key);
        if (value instanceof String s) {
            return s;
        }
        throw new ToolException("invalid argument: " + key + " must be a string");
    }

    private static long longArg(Map<?, ?> args, String key) {
        Object value = args.get(key);
        if (value instanceof Long l) {
            return l;
        }
        if (value instanceof Integer i) {
            return i.longValue();
        }
        throw new ToolException("invalid argument: " + key + " must be an integer");
    }

    private static void validateArgs(Map<?, ?> args, String... required) {
        java.util.Set<String> allowed = new java.util.TreeSet<>(List.of(required));
        for (String key : required) {
            if (!args.containsKey(key)) {
                if (isMetadataField(key)) {
                    throw new SnapshotDatabaseException("invalid_metadata", "metadata is required");
                }
                throw new ToolException("invalid argument: " + key + " is required");
            }
        }
        for (Object key : args.keySet()) {
            if (!(key instanceof String name) || !allowed.contains(name)) {
                throw new ToolException("invalid argument: unknown field");
            }
        }
    }

    private static boolean isMetadataField(String key) {
        return "kind".equals(key) || "mod_time".equals(key) || "byte_size".equals(key);
    }

    private static Map<?, ?> requireParamsObject(Object value) {
        if (value instanceof Map<?, ?> map) {
            return map;
        }
        throw new InvalidParamsException();
    }

    private static String requireParamsString(Object value) {
        if (value instanceof String s) {
            return s;
        }
        throw new InvalidParamsException();
    }

    private static Map<?, ?> requireParamsArguments(Object value) {
        if (value instanceof Map<?, ?> map) {
            return map;
        }
        throw new InvalidParamsException();
    }

    private static Map<String, Object> result(Object value) {
        return map("result", value);
    }

    private static Map<String, Object> error(int code, String message) {
        return map("error", map("code", code, "message", message));
    }

    private static Map<String, Object> ok(Object id, Object result) {
        return map("id", id, "jsonrpc", "2.0", "result", result);
    }

    private static Map<String, Object> map(Object... values) {
        Map<String, Object> map = new TreeMap<>();
        for (int i = 0; i < values.length; i += 2) {
            map.put((String) values[i], values[i + 1]);
        }
        return map;
    }

    private record Response(Map<String, Object> body, boolean shutdown) {
    }

    private static Map<String, Object> responseBody(Object id, Map<String, Object> payload) {
        Map<String, Object> body = new TreeMap<>();
        body.put("id", id);
        body.put("jsonrpc", "2.0");
        body.putAll(payload);
        return body;
    }

    private static Response response(Object id, Map<String, Object> payload) {
        return new Response(responseBody(id, payload), false);
    }

    private static final class InvalidParamsException extends RuntimeException {
    }

    private static final class ToolException extends RuntimeException {
        ToolException(String message) {
            super(message);
        }
    }
}
