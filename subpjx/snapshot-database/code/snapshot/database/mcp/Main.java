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
import java.time.Clock;
import java.time.Instant;
import java.time.LocalDateTime;
import java.time.ZoneId;
import java.time.ZoneOffset;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicLong;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public final class Main {
    private static final Pattern TIME_PATTERN = Pattern.compile(
            "^(\\d{4})-(\\d{2})-(\\d{2})_(\\d{2})-(\\d{2})-(\\d{2})_(\\d{6})Z$");
    private static final Map<String, SnapshotDatabase> DATABASES = new ConcurrentHashMap<>();
    private static final AtomicLong NEXT_DATABASE_ID = new AtomicLong(1);
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
        } catch (InvalidArgumentException e) {
            return response(id, error(-32000, "invalid argument: " + e.getMessage()));
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
            case "close-database" -> {
                SnapshotDatabase database = database(args);
                database.close();
                yield Map.of();
            }
            case "confirm-copy-completed" -> {
                database(args).confirm_copy_completed(string(args, "relative_path"), time(args, "seen_at"));
                yield Map.of();
            }
            case "generate-timestamps" -> {
                List<?> values = requireList(args.get("wall_clock_times"), "wall_clock_times");
                SequenceClock clock = new SequenceClock(values);
                SnapshotTimestampGenerator generator = new SnapshotTimestampGenerator(clock);
                java.util.ArrayList<String> timestamps = new java.util.ArrayList<>();
                for (int i = 0; i < values.size(); i++) {
                    timestamps.add(generator.next().value());
                    clock.advance();
                }
                yield Map.of("timestamps", timestamps);
            }
            case "has-rows" -> Map.of("has_rows", database(args).has_rows());
            case "lookup" -> {
                Optional<SnapshotRow> row = database(args).lookup(string(args, "relative_path"));
                yield map("found", row.isPresent(), "row", row.map(Main::row).orElse(null));
            }
            case "mark-absent" -> {
                database(args).mark_absent(string(args, "relative_path"));
                yield Map.of();
            }
            case "mark-displaced" -> {
                database(args).mark_displaced(string(args, "relative_path"));
                yield Map.of();
            }
            case "open-database" -> {
                SnapshotDatabase db = SnapshotDatabase.open(string(args, "db_file"));
                String databaseId = "db-" + NEXT_DATABASE_ID.getAndIncrement();
                DATABASES.put(databaseId, db);
                yield Map.of("database_id", databaseId);
            }
            case "path-id" -> Map.of("id", SnapshotDatabase.path_id(string(args, "relative_path")));
            case "purge" -> {
                PurgeResult result = database(args).purge(time(args, "cutoff_time"));
                yield Map.of("deleted_count", result.deleted_rows());
            }
            case "record-copy-pending" -> {
                database(args).record_copy_pending(string(args, "relative_path"), metadata(args));
                yield Map.of();
            }
            case "record-present" -> {
                database(args).record_present(string(args, "relative_path"), metadata(args), time(args, "seen_at"));
                yield Map.of();
            }
            case "root-parent-id" -> Map.of("id", SnapshotDatabase.root_parent_id());
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
        for (SnapshotDatabase database : DATABASES.values()) {
            database.close();
        }
        try {
            serverSocket.close();
        } catch (IOException ignored) {
        }
        System.exit(0);
    }

    private static SnapshotDatabase database(Map<?, ?> args) {
        SnapshotDatabase database = DATABASES.get(string(args, "database_id"));
        if (database == null) {
            throw new SnapshotDatabaseException("database_error", "database not found");
        }
        return database;
    }

    private static EntryMetadata metadata(Map<?, ?> args) {
        Object metadataValue = args.get("metadata");
        if (!(metadataValue instanceof Map<?, ?> metadata)) {
            throw new SnapshotDatabaseException("invalid_metadata", "metadata is required");
        }
        return new EntryMetadata(
                EntryKind.fromWireName(metadataString(metadata, "kind")),
                metadataTime(metadata),
                metadataLong(metadata, "byte_size"));
    }

    private static SnapshotTime metadataTime(Map<?, ?> metadata) {
        Object value = metadata.get("mod_time");
        if (!(value instanceof String text)) {
            throw new SnapshotDatabaseException("invalid_metadata", "metadata is required");
        }
        return new SnapshotTime(text);
    }

    private static String metadataString(Map<?, ?> metadata, String key) {
        Object value = metadata.get(key);
        if (value instanceof String string) {
            return string;
        }
        throw new SnapshotDatabaseException("invalid_metadata", "metadata is required");
    }

    private static long metadataLong(Map<?, ?> metadata, String key) {
        Object value = metadata.get(key);
        if (value instanceof Long number) {
            return number;
        }
        if (value instanceof Integer number) {
            return number.longValue();
        }
        if (value instanceof Short number) {
            return number.longValue();
        }
        if (value instanceof Byte number) {
            return number.longValue();
        }
        throw new SnapshotDatabaseException("invalid_metadata", "metadata is required");
    }

    private static Map<String, Object> row(SnapshotRow row) {
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
                tool("close-database", "Close an open snapshot database.", dbHandle(), emptySchema()),
                tool("confirm-copy-completed", "Confirm that a pending copy completed.", dbHandle("relative_path", "seen_at"), emptySchema()),
                tool("generate-timestamps", "Generate strictly increasing snapshot timestamps.", objectSchema(map("wall_clock_times", map("items", stringSchema(), "type", "array")), List.of("wall_clock_times")), objectSchema(map("timestamps", map("items", stringSchema(), "type", "array")), List.of("timestamps"))),
                tool("has-rows", "Report whether the snapshot table has rows.", dbHandle(), objectSchema(map("has_rows", map("type", "boolean")), List.of("has_rows"))),
                tool("lookup", "Look up one snapshot row by relative path.", dbHandle("relative_path"), lookupOutputSchema()),
                tool("mark-absent", "Mark one path absent if it exists.", dbHandle("relative_path"), emptySchema()),
                tool("mark-displaced", "Mark one path and its descendants displaced.", dbHandle("relative_path"), emptySchema()),
                tool("open-database", "Open and initialize a snapshot database.", objectSchema(map("db_file", stringSchema()), List.of("db_file")), objectSchema(map("database_id", stringSchema()), List.of("database_id"))),
                tool("path-id", "Calculate the deterministic path ID for a relative path.", objectSchema(map("relative_path", stringSchema()), List.of("relative_path")), objectSchema(map("id", stringSchema()), List.of("id"))),
                tool("purge", "Delete stale snapshot rows older than a cutoff.", dbHandle("cutoff_time"), objectSchema(map("deleted_count", map("type", "integer")), List.of("deleted_count"))),
                tool("record-copy-pending", "Record metadata for a decided file copy before completion.", dbHandle("relative_path", "metadata"), emptySchema()),
                tool("record-present", "Record an entry confirmed present.", dbHandle("relative_path", "metadata", "seen_at"), emptySchema()),
                tool("root-parent-id", "Return the root parent sentinel path ID.", objectSchema(Map.of(), List.of()), objectSchema(map("id", stringSchema()), List.of("id"))));
    }

    private static Map<String, Object> tool(String name, String description, Map<String, Object> input, Map<String, Object> output) {
        return map("description", description, "inputSchema", input, "name", name, "outputSchema", output);
    }

    private static Map<String, Object> dbHandle(String... extraRequired) {
        Map<String, Object> properties = new TreeMap<>();
        properties.put("database_id", stringSchema());
        for (String field : extraRequired) {
            switch (field) {
                case "metadata" -> properties.put("metadata", metadataSchema());
                case "seen_at", "cutoff_time" -> properties.put(field, stringSchema());
                default -> properties.put(field, stringSchema());
            }
        }
        List<String> required = new java.util.ArrayList<>();
        required.add("database_id");
        required.addAll(List.of(extraRequired));
        return objectSchema(properties, required);
    }

    private static Map<String, Object> lookupOutputSchema() {
        Map<String, Object> row = objectSchema(map(
                "basename", stringSchema(),
                "byte_size", map("type", "integer"),
                "deleted_time", map("type", List.of("string", "null")),
                "id", stringSchema(),
                "kind", stringSchema(),
                "last_seen", map("type", List.of("string", "null")),
                "mod_time", stringSchema(),
                "parent_id", stringSchema(),
                "relative_path", stringSchema()), List.of(
                "basename", "byte_size", "deleted_time", "id", "kind", "last_seen", "mod_time", "parent_id", "relative_path"));
        return objectSchema(map("found", map("type", "boolean"), "row", map("anyOf", List.of(row, map("type", "null")))), List.of("found", "row"));
    }

    private static Map<String, Object> metadataSchema() {
        return objectSchema(map(
                "byte_size", map("type", "integer"),
                "kind", map("enum", List.of("file", "directory"), "type", "string"),
                "mod_time", stringSchema()), List.of("byte_size", "kind", "mod_time"));
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

    private static SnapshotTime time(Map<?, ?> map, String key) {
        return new SnapshotTime(string(map, key));
    }

    private static String string(Map<?, ?> map, String key) {
        return requireString(map.get(key), key);
    }

    private static String requireString(Object value, String name) {
        if (value instanceof String string) {
            return string;
        }
        throw new InvalidArgumentException(name);
    }

    private static Map<?, ?> requireObject(Object value, String name) {
        if (value instanceof Map<?, ?> map) {
            return map;
        }
        throw new InvalidArgumentException(name);
    }

    private static List<?> requireList(Object value, String name) {
        if (value instanceof List<?> list) {
            return list;
        }
        throw new InvalidArgumentException(name);
    }

    private static Map<?, ?> requireParamsObject(Object value) {
        if (value instanceof Map<?, ?> map) {
            return map;
        }
        throw new InvalidParamsException();
    }

    private static String requireParamsString(Object value) {
        if (value instanceof String string) {
            return string;
        }
        throw new InvalidParamsException();
    }

    private static Map<?, ?> requireParamsArguments(Object value) {
        if (value instanceof Map<?, ?> map) {
            return map;
        }
        throw new InvalidParamsException();
    }

    private static Instant instant(String value) {
        new SnapshotTime(value);
        Matcher matcher = TIME_PATTERN.matcher(value);
        if (!matcher.matches()) {
            throw new SnapshotDatabaseException("invalid_timestamp", "invalid timestamp");
        }
        LocalDateTime dateTime = LocalDateTime.of(
                Integer.parseInt(matcher.group(1)),
                Integer.parseInt(matcher.group(2)),
                Integer.parseInt(matcher.group(3)),
                Integer.parseInt(matcher.group(4)),
                Integer.parseInt(matcher.group(5)),
                Integer.parseInt(matcher.group(6)),
                Integer.parseInt(matcher.group(7)) * 1_000);
        return dateTime.toInstant(ZoneOffset.UTC);
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

    private static Response response(Object id, Map<String, Object> payload, boolean shutdown) {
        return new Response(responseBody(id, payload), shutdown);
    }

    private static Response response(Object id, Map<String, Object> payload) {
        return response(id, payload, false);
    }

    private static final class InvalidArgumentException extends RuntimeException {
        InvalidArgumentException(String message) {
            super(message);
        }
    }

    private static final class InvalidParamsException extends RuntimeException {
    }

    private static final class ToolException extends RuntimeException {
        ToolException(String message) {
            super(message);
        }
    }

    private static final class SequenceClock extends Clock {
        private final List<?> values;
        private int index;

        SequenceClock(List<?> values) {
            this.values = values;
        }

        void advance() {
            if (index + 1 < values.size()) {
                index++;
            }
        }

        @Override
        public ZoneId getZone() {
            return ZoneOffset.UTC;
        }

        @Override
        public Clock withZone(ZoneId zone) {
            return this;
        }

        @Override
        public Instant instant() {
            return Main.instant(requireString(values.get(index), "wall_clock_times"));
        }
    }
}
