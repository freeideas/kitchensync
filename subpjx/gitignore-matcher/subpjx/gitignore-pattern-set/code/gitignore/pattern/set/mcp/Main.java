package gitignore.pattern.set.mcp;

import gitignore.pattern.set.EntryKind;
import gitignore.pattern.set.GitignorePatternSet;
import gitignore.pattern.set.GitignorePatternSetException;
import gitignore.pattern.set.PathEntry;
import gitignore.pattern.set.PatternMatch;
import gitignore.pattern.set.PatternSetSource;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ConcurrentMap;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;
import java.util.concurrent.atomic.AtomicReference;

public final class Main {
    private static final AtomicBoolean RUNNING = new AtomicBoolean(true);
    private static final AtomicReference<ServerSocket> SERVER = new AtomicReference<>();
    private static final AtomicLong NEXT_SET_ID = new AtomicLong(1);
    private static final ConcurrentMap<String, GitignorePatternSet> PATTERN_SETS = new ConcurrentHashMap<>();

    private Main() {
    }

    public static void main(String[] args) throws IOException {
        ServerSocket server = new ServerSocket(0, 50, InetAddress.getByName("127.0.0.1"));
        SERVER.set(server);
        System.out.println("MCP_PORT=" + server.getLocalPort());
        System.out.flush();
        while (RUNNING.get()) {
            try {
                Socket socket = server.accept();
                Thread handler = new Thread(() -> handle(socket), "gitignore-pattern-set-mcp");
                handler.start();
            } catch (IOException ex) {
                if (RUNNING.get()) {
                    throw ex;
                }
            }
        }
    }

    private static void handle(Socket socket) {
        try (socket;
                BufferedReader in = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
                BufferedWriter out = new BufferedWriter(new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = in.readLine()) != null && RUNNING.get()) {
                Object id = null;
                Map<String, Object> response;
                try {
                    Object parsed = Json.parse(line);
                    if (!(parsed instanceof Map<?, ?> request)) {
                        response = error(null, -32600, "invalid request");
                    } else {
                        id = request.get("id");
                        response = dispatch(request, id);
                    }
                } catch (IllegalArgumentException ex) {
                    response = error(id, -32700, "parse error");
                } catch (Exception ex) {
                    response = error(id, -32603, "internal error");
                }
                if (id != null || response != null) {
                    out.write(Json.write(response));
                    out.write('\n');
                    out.flush();
                }
            }
        } catch (IOException ignored) {
            // Connection lifecycle is owned by the caller.
        }
    }

    private static Map<String, Object> dispatch(Map<?, ?> request, Object id) {
        Object method = request.get("method");
        if (!"2.0".equals(request.get("jsonrpc")) || !(method instanceof String)) {
            return id == null ? null : error(id, -32600, "invalid request");
        }
        if (id == null) {
            if ("aitc/shutdown".equals(method)) {
                shutdown();
            }
            return null;
        }
        return switch ((String) method) {
            case "tools/list" -> {
                if (!validAbsentOrEmptyParams(request.get("params"))) {
                    yield error(id, -32602, "invalid params");
                }
                yield result(id, Map.of("tools", List.of(
                        compilePatternSetToolSchema(),
                        emptyPatternSetToolSchema(),
                        matchEntryToolSchema()
                )));
            }
            case "tools/call" -> callTool(id, request.get("params"));
            case "aitc/shutdown" -> {
                if (!validAbsentOrEmptyParams(request.get("params"))) {
                    yield error(id, -32602, "invalid params");
                }
                Map<String, Object> response = result(id, Map.of());
                shutdown();
                yield response;
            }
            default -> error(id, -32601, "method not found: " + method);
        };
    }

    private static Map<String, Object> callTool(Object id, Object params) {
        if (!(params instanceof Map<?, ?> map) || !(map.get("name") instanceof String name)
                || !(map.get("arguments") instanceof Map<?, ?> arguments)) {
            return error(id, -32602, "invalid params");
        }
        try {
            return switch (name) {
                case "compile-pattern-set" -> result(id, compilePatternSet(arguments));
                case "empty-pattern-set" -> result(id, emptyPatternSet(arguments));
                case "match-entry" -> result(id, matchEntry(arguments));
                default -> error(id, -32000, "unknown tool: " + name);
            };
        } catch (GitignorePatternSetException ex) {
            return error(id, -32000, ex.category() + ": " + ex.getMessage());
        } catch (IllegalArgumentException ex) {
            return error(id, -32000, "invalid argument: " + ex.getMessage());
        } catch (Exception ex) {
            return error(id, -32000, ex.getMessage() == null ? "tool failed" : ex.getMessage());
        }
    }

    private static Map<String, Object> compilePatternSet(Map<?, ?> arguments) {
        for (Object key : arguments.keySet()) {
            if (!"source".equals(key)) {
                throw new IllegalArgumentException(key + " is not allowed");
            }
        }
        Object value = arguments.get("source");
        if (!(value instanceof Map<?, ?> source)) {
            throw new IllegalArgumentException("source is required");
        }
        GitignorePatternSet set = GitignorePatternSet.compile(new PatternSetSource(
                requiredString(source, "pattern_text"),
                optionalString(source, "source_name")
        ));
        return storePatternSet(set);
    }

    private static Map<String, Object> emptyPatternSet(Map<?, ?> arguments) {
        if (!arguments.isEmpty()) {
            throw new IllegalArgumentException("empty-pattern-set does not accept arguments");
        }
        return storePatternSet(GitignorePatternSet.empty());
    }

    private static Map<String, Object> matchEntry(Map<?, ?> arguments) {
        for (Object key : arguments.keySet()) {
            if (!"pattern_set_id".equals(key) && !"entry".equals(key)) {
                throw new IllegalArgumentException(key + " is not allowed");
            }
        }
        String patternSetId = requiredString(arguments, "pattern_set_id");
        GitignorePatternSet set = PATTERN_SETS.get(patternSetId);
        if (set == null) {
            throw new IllegalArgumentException("pattern_set_id is unknown");
        }
        Object value = arguments.get("entry");
        if (!(value instanceof Map<?, ?> entry)) {
            throw new IllegalArgumentException("entry is required");
        }
        PatternMatch match = set.match(new PathEntry(
                requiredString(entry, "relative_path"),
                entryKind(requiredString(entry, "kind"))
        ));
        return matchOutput(match);
    }

    private static Map<String, Object> storePatternSet(GitignorePatternSet set) {
        String id = "pattern-set-" + NEXT_SET_ID.getAndIncrement();
        PATTERN_SETS.put(id, set);
        return Map.of("pattern_set_id", id);
    }

    private static EntryKind entryKind(String value) {
        try {
            return EntryKind.valueOf(value);
        } catch (IllegalArgumentException ex) {
            throw new IllegalArgumentException("kind is invalid");
        }
    }

    private static Map<String, Object> matchOutput(PatternMatch match) {
        Map<String, Object> output = new LinkedHashMap<>();
        output.put("decision", match.decision().name());
        output.put("negated", match.negated());
        if (match.source_name() != null) {
            output.put("source_name", match.source_name());
        }
        if (match.line_number() != null) {
            output.put("line_number", match.line_number());
        }
        if (match.pattern() != null) {
            output.put("pattern", match.pattern());
        }
        return output;
    }

    private static String requiredString(Map<?, ?> map, String key) {
        Object value = map.get(key);
        if (!(value instanceof String string)) {
            throw new IllegalArgumentException(key + " is required");
        }
        return string;
    }

    private static String optionalString(Map<?, ?> map, String key) {
        if (!map.containsKey(key)) {
            return null;
        }
        Object value = map.get(key);
        if (!(value instanceof String string)) {
            throw new IllegalArgumentException(key + " must be a string");
        }
        return string;
    }

    private static boolean validAbsentOrEmptyParams(Object params) {
        return params == null || (params instanceof Map<?, ?> map && map.isEmpty());
    }

    private static void shutdown() {
        RUNNING.set(false);
        ServerSocket server = SERVER.get();
        if (server != null) {
            try {
                server.close();
            } catch (IOException ignored) {
                // Closing an already closed server is harmless.
            }
        }
    }

    private static Map<String, Object> result(Object id, Map<String, Object> result) {
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("jsonrpc", "2.0");
        response.put("id", id);
        response.put("result", result);
        return response;
    }

    private static Map<String, Object> error(Object id, int code, String message) {
        Map<String, Object> err = new LinkedHashMap<>();
        err.put("code", code);
        err.put("message", message);
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("jsonrpc", "2.0");
        response.put("id", id);
        response.put("error", err);
        return response;
    }

    private static Map<String, Object> compilePatternSetToolSchema() {
        Map<String, Object> tool = new LinkedHashMap<>();
        tool.put("name", "compile-pattern-set");
        tool.put("description", "Compile gitignore pattern text into an immutable pattern set.");
        tool.put("inputSchema", objectSchema(Map.of(
                "source", objectSchema(Map.of(
                        "pattern_text", Map.of("type", "string"),
                        "source_name", Map.of("type", "string")
                ), List.of("pattern_text"))
        ), List.of("source")));
        tool.put("outputSchema", objectSchema(Map.of(
                "pattern_set_id", Map.of("type", "string")
        ), List.of("pattern_set_id")));
        return tool;
    }

    private static Map<String, Object> emptyPatternSetToolSchema() {
        Map<String, Object> tool = new LinkedHashMap<>();
        tool.put("name", "empty-pattern-set");
        tool.put("description", "Create an immutable pattern set with no patterns.");
        tool.put("inputSchema", objectSchema(Map.of(), List.of()));
        tool.put("outputSchema", objectSchema(Map.of(
                "pattern_set_id", Map.of("type", "string")
        ), List.of("pattern_set_id")));
        return tool;
    }

    private static Map<String, Object> matchEntryToolSchema() {
        Map<String, Object> tool = new LinkedHashMap<>();
        tool.put("name", "match-entry");
        tool.put("description", "Match one normalized relative path against a compiled pattern set.");
        tool.put("inputSchema", objectSchema(Map.of(
                "entry", objectSchema(Map.of(
                        "kind", enumString("regular_file", "directory", "symlink", "special"),
                        "relative_path", Map.of("type", "string")
                ), List.of("kind", "relative_path")),
                "pattern_set_id", Map.of("type", "string")
        ), List.of("entry", "pattern_set_id")));
        tool.put("outputSchema", objectSchema(Map.of(
                "decision", enumString("ignore", "include", "none"),
                "line_number", Map.of("type", "integer", "minimum", 1),
                "negated", Map.of("type", "boolean"),
                "pattern", Map.of("type", "string"),
                "source_name", Map.of("type", "string")
        ), List.of("decision", "negated")));
        return tool;
    }

    private static Map<String, Object> objectSchema(Map<String, Object> properties, List<String> required) {
        Map<String, Object> schema = new LinkedHashMap<>();
        schema.put("type", "object");
        schema.put("properties", properties);
        schema.put("required", new ArrayList<>(required));
        schema.put("additionalProperties", false);
        return schema;
    }

    private static Map<String, Object> enumString(String... values) {
        Map<String, Object> schema = new LinkedHashMap<>();
        schema.put("type", "string");
        schema.put("enum", List.of(values));
        return schema;
    }
}
