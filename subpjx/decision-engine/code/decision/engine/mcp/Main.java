package decision.engine.mcp;

import decision.engine.Action;
import decision.engine.Classification;
import decision.engine.Decision;
import decision.engine.DecisionEngine;
import decision.engine.EntryKind;
import decision.engine.HistoryRecord;
import decision.engine.Observation;
import decision.engine.Role;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

public final class Main {
    private static final String TOOL = "decide-entry";

    private Main() {
    }

    public static void main(String[] args) throws Exception {
        ServerSocket server = new ServerSocket(0, 50, java.net.InetAddress.getByName("127.0.0.1"));
        System.out.println("MCP_PORT=" + server.getLocalPort());
        System.out.flush();
        while (true) {
            Socket socket = server.accept();
            Thread thread = new Thread(() -> serve(socket));
            thread.setDaemon(false);
            thread.start();
        }
    }

    private static void serve(Socket socket) {
        try (socket;
             BufferedReader in = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
             BufferedWriter out = new BufferedWriter(new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = in.readLine()) != null) {
                Object id = null;
                try {
                    Object parsed = Json.parse(line);
                    if (!(parsed instanceof Map<?, ?> request)) {
                        write(out, error(null, -32600, "invalid request"));
                        continue;
                    }
                    id = request.get("id");
                    if (id == null) {
                        continue;
                    }
                    write(out, handle(castMap(request), id));
                } catch (IllegalArgumentException parseOrValidation) {
                    write(out, error(id, id == null ? -32700 : -32600, id == null ? "parse error" : parseOrValidation.getMessage()));
                } catch (Exception exception) {
                    write(out, error(id, -32603, "internal error"));
                }
            }
        } catch (Exception ignored) {
        }
    }

    private static Map<String, Object> handle(Map<String, Object> request, Object id) {
        if (!"2.0".equals(request.get("jsonrpc")) || !(request.get("method") instanceof String method)) {
            return error(id, -32600, "invalid request");
        }
        return switch (method) {
            case "tools/list" -> result(id, toolsList());
            case "tools/call" -> callTool(id, request.get("params"));
            default -> error(id, -32601, "method not found: " + method);
        };
    }

    private static Map<String, Object> callTool(Object id, Object paramsObject) {
        if (!(paramsObject instanceof Map<?, ?> paramsRaw)) {
            return error(id, -32602, "invalid params");
        }
        Map<String, Object> params = castMap(paramsRaw);
        if (!(params.get("name") instanceof String name) || !(params.get("arguments") instanceof Map<?, ?> argumentsRaw)) {
            return error(id, -32602, "invalid params");
        }
        if (!TOOL.equals(name)) {
            return error(id, -32000, "not implemented");
        }
        try {
            Map<String, Object> arguments = castMap(argumentsRaw);
            Decision decision = DecisionEngine.decideEntry(
                    parseRoles(requiredMap(arguments, "roles")),
                    parseObservations(requiredMap(arguments, "observations")),
                    parseHistories(optionalMap(arguments, "histories")),
                    optionalLong(arguments, "tolerance", DecisionEngine.DEFAULT_TOLERANCE_SECONDS));
            return result(id, decisionToJson(decision));
        } catch (IllegalArgumentException exception) {
            return error(id, -32000, "invalid argument: " + exception.getMessage());
        } catch (Exception exception) {
            return error(id, -32000, exception.getMessage() == null ? "tool error" : exception.getMessage());
        }
    }

    private static Map<String, Role> parseRoles(Map<String, Object> raw) {
        TreeMap<String, Role> roles = new TreeMap<>();
        for (Map.Entry<String, Object> entry : raw.entrySet()) {
            if (!(entry.getValue() instanceof String role)) {
                throw new IllegalArgumentException("role for " + entry.getKey() + " must be a string");
            }
            roles.put(entry.getKey(), switch (role) {
                case "canon" -> Role.CANON;
                case "contributing" -> Role.CONTRIBUTING;
                case "subordinate" -> Role.SUBORDINATE;
                default -> throw new IllegalArgumentException("unknown role: " + role);
            });
        }
        return roles;
    }

    private static Map<String, Observation> parseObservations(Map<String, Object> raw) {
        TreeMap<String, Observation> observations = new TreeMap<>();
        for (Map.Entry<String, Object> entry : raw.entrySet()) {
            Map<String, Object> object = asMap(entry.getValue(), "observation for " + entry.getKey());
            String kind = requiredString(object, "kind");
            observations.put(entry.getKey(), switch (kind) {
                case "File" -> Observation.file(requiredLong(object, "mod_time"), requiredLong(object, "byte_size"));
                case "Directory" -> Observation.directory();
                case "Absent" -> Observation.absent();
                default -> throw new IllegalArgumentException("unknown observation kind: " + kind);
            });
        }
        return observations;
    }

    private static Map<String, HistoryRecord> parseHistories(Map<String, Object> raw) {
        TreeMap<String, HistoryRecord> histories = new TreeMap<>();
        for (Map.Entry<String, Object> entry : raw.entrySet()) {
            Map<String, Object> object = asMap(entry.getValue(), "history for " + entry.getKey());
            histories.put(entry.getKey(), new HistoryRecord(
                    requiredLong(object, "mod_time"),
                    requiredLong(object, "byte_size"),
                    nullableLong(object, "last_seen"),
                    nullableLong(object, "deleted_time")));
        }
        return histories;
    }

    private static Map<String, Object> decisionToJson(Decision decision) {
        LinkedHashMap<String, Object> out = new LinkedHashMap<>();
        out.put("actions", actionsToJson(decision.actions()));
        out.put("classifications", classificationsToJson(decision.classifications()));
        out.put("entry_kind", entryKind(decision.entryKind()));
        if (decision.winningByteSize() != null) {
            out.put("winning_byte_size", decision.winningByteSize());
        }
        if (decision.winningModTime() != null) {
            out.put("winning_mod_time", decision.winningModTime());
        }
        if (decision.winningSource() != null) {
            out.put("winning_source", decision.winningSource());
        }
        return out;
    }

    private static Map<String, Object> actionsToJson(Map<String, Action> actions) {
        TreeMap<String, Object> out = new TreeMap<>();
        for (Map.Entry<String, Action> entry : actions.entrySet()) {
            LinkedHashMap<String, Object> action = new LinkedHashMap<>();
            action.put("kind", actionKind(entry.getValue().kind()));
            if (entry.getValue().source() != null) {
                action.put("source", entry.getValue().source());
            }
            out.put(entry.getKey(), action);
        }
        return out;
    }

    private static Map<String, Object> classificationsToJson(Map<String, Classification> classifications) {
        TreeMap<String, Object> out = new TreeMap<>();
        for (Map.Entry<String, Classification> entry : classifications.entrySet()) {
            out.put(entry.getKey(), classification(entry.getValue()));
        }
        return out;
    }

    private static String entryKind(EntryKind kind) {
        return switch (kind) {
            case FILE -> "File";
            case DIRECTORY -> "Directory";
            case NONE -> "None";
        };
    }

    private static String actionKind(Action.Kind kind) {
        return switch (kind) {
            case NO_OP -> "NoOp";
            case RECEIVE_FILE -> "ReceiveFile";
            case CREATE_DIRECTORY -> "CreateDirectory";
            case DISPLACE -> "Displace";
        };
    }

    private static String classification(Classification classification) {
        return switch (classification) {
            case UNCHANGED -> "Unchanged";
            case MODIFIED -> "Modified";
            case RESURRECTED -> "Resurrected";
            case NEW -> "New";
            case DELETED -> "Deleted";
            case ABSENT_UNCONFIRMED -> "AbsentUnconfirmed";
            case NO_OPINION -> "NoOpinion";
        };
    }

    private static Map<String, Object> toolsList() {
        return object("tools", List.of(tool()));
    }

    private static Map<String, Object> tool() {
        LinkedHashMap<String, Object> tool = new LinkedHashMap<>();
        tool.put("description", "Decide the authoritative state for one entry.");
        tool.put("inputSchema", inputSchema());
        tool.put("name", TOOL);
        tool.put("outputSchema", outputSchema());
        return tool;
    }

    private static Map<String, Object> inputSchema() {
        return object(
                "additionalProperties", false,
                "properties", object(
                        "histories", participantMap(historySchema()),
                        "observations", participantMap(observationSchema()),
                        "roles", object("additionalProperties", object("enum", List.of("canon", "contributing", "subordinate"), "type", "string"), "type", "object"),
                        "tolerance", object("minimum", 0, "type", "integer")),
                "required", List.of("roles", "observations"),
                "type", "object");
    }

    private static Map<String, Object> outputSchema() {
        return object(
                "additionalProperties", false,
                "properties", object(
                        "actions", participantMap(actionSchema()),
                        "classifications", object("additionalProperties", object("enum", List.of("Unchanged", "Modified", "Resurrected", "New", "Deleted", "AbsentUnconfirmed", "NoOpinion"), "type", "string"), "type", "object"),
                        "entry_kind", object("enum", List.of("File", "Directory", "None"), "type", "string"),
                        "winning_byte_size", object("type", "integer"),
                        "winning_mod_time", object("type", "integer"),
                        "winning_source", object("type", "string")),
                "required", List.of("actions", "classifications", "entry_kind"),
                "type", "object");
    }

    private static Map<String, Object> observationSchema() {
        return object(
                "additionalProperties", false,
                "properties", object(
                        "byte_size", object("type", "integer"),
                        "kind", object("enum", List.of("File", "Directory", "Absent"), "type", "string"),
                        "mod_time", object("type", "integer")),
                "required", List.of("kind"),
                "type", "object");
    }

    private static Map<String, Object> historySchema() {
        return object(
                "additionalProperties", false,
                "properties", object(
                        "byte_size", object("type", "integer"),
                        "deleted_time", object("type", List.of("integer", "null")),
                        "last_seen", object("type", List.of("integer", "null")),
                        "mod_time", object("type", "integer")),
                "required", List.of("mod_time", "byte_size", "last_seen", "deleted_time"),
                "type", "object");
    }

    private static Map<String, Object> actionSchema() {
        return object(
                "additionalProperties", false,
                "properties", object(
                        "kind", object("enum", List.of("NoOp", "ReceiveFile", "CreateDirectory", "Displace"), "type", "string"),
                        "source", object("type", "string")),
                "required", List.of("kind"),
                "type", "object");
    }

    private static Map<String, Object> participantMap(Map<String, Object> valueSchema) {
        return object("additionalProperties", valueSchema, "type", "object");
    }

    private static Map<String, Object> result(Object id, Object result) {
        return object("id", id, "jsonrpc", "2.0", "result", result);
    }

    private static Map<String, Object> error(Object id, int code, String message) {
        return object("error", object("code", code, "message", message), "id", id, "jsonrpc", "2.0");
    }

    private static void write(BufferedWriter out, Map<String, Object> response) throws Exception {
        out.write(Json.stringify(response));
        out.write('\n');
        out.flush();
    }

    private static Map<String, Object> object(Object... pairs) {
        LinkedHashMap<String, Object> object = new LinkedHashMap<>();
        for (int i = 0; i < pairs.length; i += 2) {
            object.put((String) pairs[i], pairs[i + 1]);
        }
        return object;
    }

    private static Map<String, Object> requiredMap(Map<String, Object> object, String key) {
        return asMap(object.get(key), key);
    }

    private static Map<String, Object> optionalMap(Map<String, Object> object, String key) {
        Object value = object.get(key);
        return value == null ? Map.of() : asMap(value, key);
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> asMap(Object value, String name) {
        if (!(value instanceof Map<?, ?> map)) {
            throw new IllegalArgumentException(name + " must be an object");
        }
        return (Map<String, Object>) map;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> castMap(Map<?, ?> map) {
        return (Map<String, Object>) map;
    }

    private static String requiredString(Map<String, Object> object, String key) {
        Object value = object.get(key);
        if (!(value instanceof String string)) {
            throw new IllegalArgumentException(key + " must be a string");
        }
        return string;
    }

    private static long requiredLong(Map<String, Object> object, String key) {
        Object value = object.get(key);
        if (!(value instanceof Number number)) {
            throw new IllegalArgumentException(key + " must be an integer");
        }
        return number.longValue();
    }

    private static Long nullableLong(Map<String, Object> object, String key) {
        Object value = object.get(key);
        if (value == null) {
            return null;
        }
        if (!(value instanceof Number number)) {
            throw new IllegalArgumentException(key + " must be an integer or null");
        }
        return number.longValue();
    }

    private static long optionalLong(Map<String, Object> object, String key, long defaultValue) {
        Object value = object.get(key);
        if (value == null) {
            return defaultValue;
        }
        if (!(value instanceof Number number)) {
            throw new IllegalArgumentException(key + " must be an integer");
        }
        return number.longValue();
    }
}
