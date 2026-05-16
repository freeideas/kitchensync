package sync.decision.engine.mcp;

import sync.decision.engine.AuthoritativeKind;
import sync.decision.engine.AuthoritativeState;
import sync.decision.engine.DecisionInput;
import sync.decision.engine.EntryDecision;
import sync.decision.engine.EntryKind;
import sync.decision.engine.FilesystemEffect;
import sync.decision.engine.InvalidInputException;
import sync.decision.engine.LiveEntry;
import sync.decision.engine.PeerId;
import sync.decision.engine.PeerRole;
import sync.decision.engine.SnapshotEffect;
import sync.decision.engine.SnapshotRow;
import sync.decision.engine.SyncDecisionEngine;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class Main {
    private static final String TOOL_DECIDE_ENTRY = "decide-entry";
    private static volatile ServerSocket serverSocket;

    private Main() {
    }

    public static void main(String[] args) throws IOException {
        serverSocket = new ServerSocket(0, 50, InetAddress.getByName("127.0.0.1"));
        System.out.println("MCP_PORT=" + serverSocket.getLocalPort());
        System.out.flush();

        ExecutorService executor = Executors.newCachedThreadPool();
        while (!serverSocket.isClosed()) {
            try {
                Socket socket = serverSocket.accept();
                executor.execute(() -> serve(socket));
            } catch (IOException e) {
                if (!serverSocket.isClosed()) {
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
            while ((line = reader.readLine()) != null) {
                Response response = handleLine(line);
                if (response == null) {
                    continue;
                }
                writer.write(Json.write(response.body()));
                writer.write('\n');
                writer.flush();
                if (response.shutdown()) {
                    shutdown();
                }
            }
        } catch (IOException ignored) {
        }
    }

    private static Response handleLine(String line) {
        Object parsed;
        try {
            parsed = Json.parse(line);
        } catch (InvalidInputException e) {
            return response(null, error(-32000, "invalid_input"));
        } catch (RuntimeException e) {
            return response(null, error(-32700, "parse error"));
        }
        if (!(parsed instanceof Map<?, ?> request)) {
            return response(null, error(-32600, "invalid request"));
        }

        Object id = request.get("id");
        if (!request.containsKey("id")) {
            if ("aitc/shutdown".equals(request.get("method"))) {
                return new Response(null, true);
            }
            return null;
        }
        if (!"2.0".equals(request.get("jsonrpc")) || !(request.get("method") instanceof String method)) {
            return response(id, error(-32600, "invalid request"));
        }

        try {
            return switch (method) {
                case "tools/list" -> handleToolsList(id, request.get("params"));
                case "tools/call" -> handleToolsCall(id, request.get("params"));
                case "aitc/shutdown" -> handleShutdown(id, request.get("params"));
                default -> response(id, error(-32601, "method not found: " + method));
            };
        } catch (InvalidArgumentException e) {
            return response(id, error(-32000, "invalid argument: " + e.getMessage()));
        } catch (InvalidInputException e) {
            return response(id, error(-32000, "invalid_input"));
        } catch (RuntimeException e) {
            return response(id, error(-32603, "internal error"));
        }
    }

    private static Response handleToolsList(Object id, Object params) {
        if (params != null) {
            requireObject(params, "params");
        }
        return response(id, result(map("tools", List.of(toolSchema()))));
    }

    private static Response handleToolsCall(Object id, Object params) {
        if (!(params instanceof Map<?, ?> object)
                || !(object.get("name") instanceof String name)
                || !(object.get("arguments") instanceof Map<?, ?>)) {
            return response(id, error(-32602, "invalid params"));
        }
        Object arguments = object.get("arguments");
        if (!TOOL_DECIDE_ENTRY.equals(name)) {
            return response(id, error(-32602, "invalid params"));
        }
        DecisionInput input = parseInput(requireObject(arguments, "arguments"));
        return response(id, result(decisionToJson(SyncDecisionEngine.decideEntry(input))));
    }

    private static Response handleShutdown(Object id, Object params) {
        if (params instanceof Map<?, ?> object) {
            if (!object.isEmpty()) {
                return response(id, error(-32602, "invalid params"));
            }
        } else if (params != null) {
            return response(id, error(-32602, "invalid params"));
        }
        return new Response(ok(id, Map.of()), true);
    }

    private static void shutdown() {
        try {
            serverSocket.close();
        } catch (IOException ignored) {
        }
        System.exit(0);
    }

    private static DecisionInput parseInput(Map<?, ?> args) {
        String relativePath = requireString(args.get("relative_path"), "relative_path");
        LinkedHashMap<PeerId, PeerRole> peers = parsePeers(requireObject(args.get("peers"), "peers"));
        Map<PeerId, LiveEntry> liveEntries = parseLiveEntries(requireObject(args.get("live_entries"), "live_entries"));
        Map<PeerId, SnapshotRow> snapshotRows = parseSnapshotRows(requireObject(args.get("snapshot_rows"), "snapshot_rows"));
        return new DecisionInput(relativePath, peers, liveEntries, snapshotRows);
    }

    private static LinkedHashMap<PeerId, PeerRole> parsePeers(Map<?, ?> values) {
        LinkedHashMap<PeerId, PeerRole> peers = new LinkedHashMap<>();
        for (var entry : values.entrySet()) {
            PeerId id = new PeerId(requireString(entry.getKey(), "peer id"));
            peers.put(id, parseRole(requireString(entry.getValue(), "role")));
        }
        return peers;
    }

    private static Map<PeerId, LiveEntry> parseLiveEntries(Map<?, ?> values) {
        LinkedHashMap<PeerId, LiveEntry> entries = new LinkedHashMap<>();
        for (var entry : values.entrySet()) {
            PeerId peer = new PeerId(requireString(entry.getKey(), "peer id"));
            Map<?, ?> row = requireObject(entry.getValue(), "live entry");
            entries.put(peer, new LiveEntry(
                    parseKind(requireString(row.get("kind"), "kind")),
                    parseInstant(requireString(row.get("mod_time"), "mod_time")),
                    requireLong(row.get("byte_size"), "byte_size")));
        }
        return entries;
    }

    private static Map<PeerId, SnapshotRow> parseSnapshotRows(Map<?, ?> values) {
        LinkedHashMap<PeerId, SnapshotRow> rows = new LinkedHashMap<>();
        for (var entry : values.entrySet()) {
            PeerId peer = new PeerId(requireString(entry.getKey(), "peer id"));
            Map<?, ?> row = requireObject(entry.getValue(), "snapshot row");
            rows.put(peer, new SnapshotRow(
                    parseKind(requireString(row.get("kind"), "kind")),
                    parseInstant(requireString(row.get("mod_time"), "mod_time")),
                    requireLong(row.get("byte_size"), "byte_size"),
                    optionalInstant(row.get("last_seen")),
                    optionalInstant(row.get("deleted_time"))));
        }
        return rows;
    }

    private static PeerRole parseRole(String value) {
        try {
            return PeerRole.fromWireName(value);
        } catch (IllegalArgumentException e) {
            throw new InvalidArgumentException("role");
        }
    }

    private static EntryKind parseKind(String value) {
        try {
            return EntryKind.fromWireName(value);
        } catch (IllegalArgumentException e) {
            throw new InvalidArgumentException("kind");
        }
    }

    private static Instant parseInstant(String value) {
        try {
            return Instant.parse(value);
        } catch (RuntimeException e) {
            throw new InvalidArgumentException("instant");
        }
    }

    private static Instant optionalInstant(Object value) {
        if (value == null) {
            return null;
        }
        return parseInstant(requireString(value, "instant"));
    }

    private static Map<String, Object> decisionToJson(EntryDecision decision) {
        return map(
                "authoritative_state", stateToJson(decision.authoritativeState()),
                "filesystem_effects", filesystemToJson(decision),
                "recurse_peers", decision.recursePeers().stream().map(PeerId::value).toList(),
                "skipped", decision.skipped(),
                "snapshot_effects", snapshotToJson(decision));
    }

    private static Map<String, Object> stateToJson(AuthoritativeState state) {
        return map(
                "byte_size", state.byteSize(),
                "kind", state.kind().wireName(),
                "mod_time", state.modTime() == null ? null : state.modTime().toString(),
                "source_peer", state.sourcePeer() == null ? null : state.sourcePeer().value());
    }

    private static Map<String, Object> filesystemToJson(EntryDecision decision) {
        LinkedHashMap<String, Object> rows = new LinkedHashMap<>();
        String source = decision.authoritativeState().sourcePeer() == null ? null : decision.authoritativeState().sourcePeer().value();
        for (var peer : decision.filesystemEffects().entrySet()) {
            List<Object> effects = new ArrayList<>();
            for (FilesystemEffect effect : peer.getValue()) {
                effects.add(map("effect", effect.wireName(), "source_peer", effect == FilesystemEffect.COPY_FILE ? source : null));
            }
            rows.put(peer.getKey().value(), effects);
        }
        return rows;
    }

    private static Map<String, Object> snapshotToJson(EntryDecision decision) {
        LinkedHashMap<String, Object> rows = new LinkedHashMap<>();
        for (var peer : decision.snapshotEffects().entrySet()) {
            rows.put(peer.getKey().value(), peer.getValue().stream().map(SnapshotEffect::wireName).toList());
        }
        return rows;
    }

    private static Map<String, Object> toolSchema() {
        return map(
                "description", "Decide synchronization effects for one relative path.",
                "inputSchema", inputSchema(),
                "name", TOOL_DECIDE_ENTRY,
                "outputSchema", outputSchema());
    }

    private static Map<String, Object> inputSchema() {
        return map(
                "additionalProperties", false,
                "properties", map(
                        "live_entries", map("additionalProperties", liveEntrySchema(), "type", "object"),
                        "peers", map("additionalProperties", peerRoleSchema(), "type", "object"),
                        "relative_path", map("type", "string"),
                        "snapshot_rows", map("additionalProperties", snapshotRowSchema(), "type", "object")),
                "required", List.of("relative_path", "peers", "live_entries", "snapshot_rows"),
                "type", "object");
    }

    private static Map<String, Object> outputSchema() {
        return map(
                "additionalProperties", false,
                "properties", map(
                        "authoritative_state", stateSchema(),
                        "filesystem_effects", map("additionalProperties", map("items", filesystemEffectSchema(), "type", "array"), "type", "object"),
                        "recurse_peers", map("items", map("type", "string"), "type", "array"),
                        "skipped", map("type", "boolean"),
                        "snapshot_effects", map("additionalProperties", map("items", snapshotEffectSchema(), "type", "array"), "type", "object")),
                "required", List.of("authoritative_state", "filesystem_effects", "snapshot_effects", "recurse_peers", "skipped"),
                "type", "object");
    }

    private static Map<String, Object> peerRoleSchema() {
        return map("enum", List.of("canon", "normal", "subordinate"), "type", "string");
    }

    private static Map<String, Object> liveEntrySchema() {
        return map(
                "additionalProperties", false,
                "properties", map(
                        "byte_size", map("type", "integer"),
                        "kind", map("enum", List.of("file", "directory"), "type", "string"),
                        "mod_time", map("type", "string")),
                "required", List.of("kind", "mod_time", "byte_size"),
                "type", "object");
    }

    private static Map<String, Object> snapshotRowSchema() {
        return map(
                "additionalProperties", false,
                "properties", map(
                        "byte_size", map("type", "integer"),
                        "deleted_time", map("type", List.of("string", "null")),
                        "kind", map("enum", List.of("file", "directory"), "type", "string"),
                        "last_seen", map("type", List.of("string", "null")),
                        "mod_time", map("type", "string")),
                "required", List.of("kind", "mod_time", "byte_size"),
                "type", "object");
    }

    private static Map<String, Object> stateSchema() {
        return map(
                "additionalProperties", false,
                "properties", map(
                        "byte_size", map("type", List.of("integer", "null")),
                        "kind", map("enum", List.of("absent", "file", "directory"), "type", "string"),
                        "mod_time", map("type", List.of("string", "null")),
                        "source_peer", map("type", List.of("string", "null"))),
                "required", List.of("kind", "source_peer", "mod_time", "byte_size"),
                "type", "object");
    }

    private static Map<String, Object> filesystemEffectSchema() {
        return map(
                "additionalProperties", false,
                "properties", map(
                        "effect", map("enum", List.of("keep", "copy_file", "create_directory", "displace"), "type", "string"),
                        "source_peer", map("type", List.of("string", "null"))),
                "required", List.of("effect", "source_peer"),
                "type", "object");
    }

    private static Map<String, Object> snapshotEffectSchema() {
        return map("enum", List.of("confirm_present", "copy_pending", "create_directory_confirmed", "mark_absent", "mark_displaced", "no_snapshot_change"), "type", "string");
    }

    private static Map<?, ?> requireObject(Object value, String name) {
        if (value instanceof Map<?, ?> object) {
            return object;
        }
        throw new InvalidArgumentException(name);
    }

    private static String requireString(Object value, String name) {
        if (value instanceof String string) {
            return string;
        }
        throw new InvalidArgumentException(name);
    }

    private static long requireLong(Object value, String name) {
        if (value instanceof Number number && Math.rint(number.doubleValue()) == number.doubleValue()) {
            return number.longValue();
        }
        throw new InvalidArgumentException(name);
    }

    private static Map<String, Object> ok(Object id, Object result) {
        return map("id", id, "jsonrpc", "2.0", "result", result);
    }

    private static Map<String, Object> result(Object result) {
        return map("result", result);
    }

    private static Map<String, Object> error(int code, String message) {
        return map("error", map("code", code, "message", message));
    }

    private static Response response(Object id, Map<String, Object> payload) {
        if (payload.containsKey("result")) {
            return new Response(ok(id, payload.get("result")), false);
        }
        return new Response(map("error", payload.get("error"), "id", id, "jsonrpc", "2.0"), false);
    }

    private static Map<String, Object> map(Object... values) {
        TreeMap<String, Object> map = new TreeMap<>();
        for (int i = 0; i < values.length; i += 2) {
            map.put((String) values[i], values[i + 1]);
        }
        return map;
    }

    private record Response(Map<String, Object> body, boolean shutdown) {
    }

    private static final class InvalidArgumentException extends RuntimeException {
        InvalidArgumentException(String message) {
            super(message);
        }
    }

    private static final class Json {
        private final String text;
        private int index;

        private Json(String text) {
            this.text = text;
        }

        static Object parse(String text) {
            Json parser = new Json(text);
            Object value = parser.readValue();
            parser.skipWhitespace();
            if (parser.index != parser.text.length()) {
                throw new IllegalArgumentException();
            }
            return value;
        }

        static String write(Object value) {
            StringBuilder builder = new StringBuilder();
            writeValue(builder, value);
            return builder.toString();
        }

        private Object readValue() {
            skipWhitespace();
            if (index >= text.length()) {
                throw new IllegalArgumentException();
            }
            char c = text.charAt(index);
            if (c == '"') {
                return readString();
            }
            if (c == '{') {
                return readObject();
            }
            if (c == '[') {
                return readArray();
            }
            if (c == '-' || Character.isDigit(c)) {
                return readNumber();
            }
            if (text.startsWith("true", index)) {
                index += 4;
                return Boolean.TRUE;
            }
            if (text.startsWith("false", index)) {
                index += 5;
                return Boolean.FALSE;
            }
            if (text.startsWith("null", index)) {
                index += 4;
                return null;
            }
            throw new IllegalArgumentException();
        }

        private Map<String, Object> readObject() {
            index++;
            LinkedHashMap<String, Object> object = new LinkedHashMap<>();
            skipWhitespace();
            if (consume('}')) {
                return object;
            }
            while (true) {
                skipWhitespace();
                String key = readString();
                skipWhitespace();
                require(':');
                if (object.containsKey(key)) {
                    throw new InvalidInputException();
                }
                object.put(key, readValue());
                skipWhitespace();
                if (consume('}')) {
                    return object;
                }
                require(',');
            }
        }

        private List<Object> readArray() {
            index++;
            ArrayList<Object> values = new ArrayList<>();
            skipWhitespace();
            if (consume(']')) {
                return values;
            }
            while (true) {
                values.add(readValue());
                skipWhitespace();
                if (consume(']')) {
                    return values;
                }
                require(',');
            }
        }

        private String readString() {
            require('"');
            StringBuilder builder = new StringBuilder();
            while (index < text.length()) {
                char c = text.charAt(index++);
                if (c == '"') {
                    return builder.toString();
                }
                if (c == '\\') {
                    if (index >= text.length()) {
                        throw new IllegalArgumentException();
                    }
                    char escaped = text.charAt(index++);
                    switch (escaped) {
                        case '"' -> builder.append('"');
                        case '\\' -> builder.append('\\');
                        case '/' -> builder.append('/');
                        case 'b' -> builder.append('\b');
                        case 'f' -> builder.append('\f');
                        case 'n' -> builder.append('\n');
                        case 'r' -> builder.append('\r');
                        case 't' -> builder.append('\t');
                        case 'u' -> {
                            if (index + 4 > text.length()) {
                                throw new IllegalArgumentException();
                            }
                            builder.append((char) Integer.parseInt(text.substring(index, index + 4), 16));
                            index += 4;
                        }
                        default -> throw new IllegalArgumentException();
                    }
                } else {
                    builder.append(c);
                }
            }
            throw new IllegalArgumentException();
        }

        private Number readNumber() {
            int start = index;
            if (consume('-') && index >= text.length()) {
                throw new IllegalArgumentException();
            }
            while (index < text.length() && Character.isDigit(text.charAt(index))) {
                index++;
            }
            if (index < text.length() && text.charAt(index) == '.') {
                index++;
                while (index < text.length() && Character.isDigit(text.charAt(index))) {
                    index++;
                }
            }
            if (index < text.length() && (text.charAt(index) == 'e' || text.charAt(index) == 'E')) {
                index++;
                if (index < text.length() && (text.charAt(index) == '+' || text.charAt(index) == '-')) {
                    index++;
                }
                while (index < text.length() && Character.isDigit(text.charAt(index))) {
                    index++;
                }
            }
            String number = text.substring(start, index);
            if (number.contains(".") || number.contains("e") || number.contains("E")) {
                return Double.parseDouble(number);
            }
            return Long.parseLong(number);
        }

        private void skipWhitespace() {
            while (index < text.length() && Character.isWhitespace(text.charAt(index))) {
                index++;
            }
        }

        private boolean consume(char expected) {
            if (index < text.length() && text.charAt(index) == expected) {
                index++;
                return true;
            }
            return false;
        }

        private void require(char expected) {
            if (!consume(expected)) {
                throw new IllegalArgumentException();
            }
        }

        private static void writeValue(StringBuilder builder, Object value) {
            if (value == null) {
                builder.append("null");
            } else if (value instanceof String string) {
                writeString(builder, string);
            } else if (value instanceof Number || value instanceof Boolean) {
                builder.append(value);
            } else if (value instanceof Map<?, ?> map) {
                writeObject(builder, map);
            } else if (value instanceof Iterable<?> iterable) {
                writeArray(builder, iterable);
            } else {
                writeString(builder, value.toString());
            }
        }

        private static void writeObject(StringBuilder builder, Map<?, ?> map) {
            builder.append('{');
            List<Map.Entry<?, ?>> entries = new ArrayList<>(map.entrySet());
            entries.sort(Comparator.comparing(entry -> entry.getKey().toString()));
            for (int i = 0; i < entries.size(); i++) {
                if (i > 0) {
                    builder.append(',');
                }
                writeString(builder, entries.get(i).getKey().toString());
                builder.append(':');
                writeValue(builder, entries.get(i).getValue());
            }
            builder.append('}');
        }

        private static void writeArray(StringBuilder builder, Iterable<?> values) {
            builder.append('[');
            boolean first = true;
            for (Object value : values) {
                if (!first) {
                    builder.append(',');
                }
                first = false;
                writeValue(builder, value);
            }
            builder.append(']');
        }

        private static void writeString(StringBuilder builder, String value) {
            builder.append('"');
            for (int i = 0; i < value.length(); i++) {
                char c = value.charAt(i);
                switch (c) {
                    case '"' -> builder.append("\\\"");
                    case '\\' -> builder.append("\\\\");
                    case '\b' -> builder.append("\\b");
                    case '\f' -> builder.append("\\f");
                    case '\n' -> builder.append("\\n");
                    case '\r' -> builder.append("\\r");
                    case '\t' -> builder.append("\\t");
                    default -> {
                        if (c < 0x20) {
                            builder.append(String.format("\\u%04x", (int) c));
                        } else {
                            builder.append(c);
                        }
                    }
                }
            }
            builder.append('"');
        }
    }
}
