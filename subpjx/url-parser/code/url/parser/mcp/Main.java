package url.parser.mcp;

import url.parser.ParseContext;
import url.parser.ParsedPeer;
import url.parser.ParsedUrl;
import url.parser.PeerUrlParser;
import url.parser.UrlParseException;
import url.parser.UrlSettings;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.concurrent.atomic.AtomicBoolean;

public final class Main {
    private static final AtomicBoolean RUNNING = new AtomicBoolean(true);
    private static ServerSocket server;

    private Main() {
    }

    public static void main(String[] args) throws IOException {
        server = new ServerSocket(0, 50, java.net.InetAddress.getByName("127.0.0.1"));
        System.out.println("MCP_PORT=" + server.getLocalPort());
        System.out.flush();
        while (RUNNING.get()) {
            try {
                Socket socket = server.accept();
                Thread.ofPlatform().start(() -> serve(socket));
            } catch (IOException ex) {
                if (RUNNING.get()) {
                    throw ex;
                }
            }
        }
    }

    private static void serve(Socket socket) {
        try (socket;
             BufferedReader reader = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
             BufferedWriter writer = new BufferedWriter(new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null && RUNNING.get()) {
                Object id = null;
                try {
                    Object parsed = Json.parse(line);
                    if (!(parsed instanceof Map<?, ?> request)) {
                        write(writer, error(null, -32600, "invalid request"));
                        continue;
                    }
                    id = request.get("id");
                    if (id == null) {
                        continue;
                    }
                    write(writer, handle(request, id));
                } catch (JsonException ex) {
                    write(writer, error(id, -32700, "parse error"));
                } catch (Exception ex) {
                    write(writer, error(id, -32603, "internal error"));
                }
            }
        } catch (IOException ignored) {
            // Connection lifecycle is owned by the caller.
        }
    }

    private static Map<String, Object> handle(Map<?, ?> request, Object id) throws IOException {
        if (!"2.0".equals(request.get("jsonrpc")) || !(request.get("method") instanceof String method)) {
            return error(id, -32600, "invalid request");
        }
        return switch (method) {
            case "tools/list" -> result(id, toolsList());
            case "tools/call" -> callTool(id, request.get("params"));
            case "aitc/shutdown" -> shutdown(id, request.get("params"));
            default -> error(id, -32601, "method not found: " + method);
        };
    }

    private static Map<String, Object> callTool(Object id, Object paramsValue) {
        if (!(paramsValue instanceof Map<?, ?> params)
                || !(params.get("name") instanceof String name)
                || !(params.get("arguments") instanceof Map<?, ?> arguments)) {
            return error(id, -32602, "invalid params");
        }
        try {
            ParseContext context = context(arguments.get("context"));
            Object textValue = arguments.get("text");
            if (!(textValue instanceof String text)) {
                return toolError(id, "invalid argument: text is required");
            }
            return switch (name) {
                case "normalize-identity" -> result(id, Map.of("canonical_identity", PeerUrlParser.normalize_identity(text, context)));
                case "parse-peer-operand" -> result(id, peer(PeerUrlParser.parse_peer_operand(text, context)));
                case "parse-url" -> result(id, url(PeerUrlParser.parse_url(text, context)));
                default -> toolError(id, "not implemented");
            };
        } catch (IllegalArgumentException ex) {
            return toolError(id, "invalid argument: " + ex.getMessage());
        } catch (UrlParseException ex) {
            return toolError(id, ex.category().name());
        }
    }

    private static ParseContext context(Object value) {
        if (!(value instanceof Map<?, ?> context)) {
            throw new IllegalArgumentException("context is required");
        }
        Object cwd = context.get("current_working_directory");
        Object user = context.get("current_os_user");
        if (!(cwd instanceof String cwdText) || !(user instanceof String userText)) {
            throw new IllegalArgumentException("context fields are required");
        }
        return new ParseContext(cwdText, userText);
    }

    private static Map<String, Object> shutdown(Object id, Object params) throws IOException {
        if (params != null && !(params instanceof Map<?, ?> map && map.isEmpty())) {
            return error(id, -32602, "invalid params");
        }
        RUNNING.set(false);
        Map<String, Object> response = result(id, Map.of());
        server.close();
        return response;
    }

    private static Map<String, Object> peer(ParsedPeer peer) {
        return ordered(Map.of(
                "role", peer.role().name(),
                "candidates", peer.candidates().stream().map(Main::url).toList()));
    }

    private static Map<String, Object> url(ParsedUrl parsed) {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("canonical_identity", parsed.canonical_identity());
        out.put("endpoint_key", parsed.endpoint_key());
        out.put("host", parsed.host());
        out.put("password", parsed.password());
        out.put("path", parsed.path());
        out.put("port", parsed.port());
        out.put("scheme", parsed.scheme().name());
        out.put("settings", settings(parsed.settings()));
        out.put("user", parsed.user());
        out.entrySet().removeIf(entry -> entry.getValue() == null);
        return ordered(out);
    }

    private static Map<String, Object> settings(UrlSettings settings) {
        Map<String, Object> out = new LinkedHashMap<>();
        if (settings.connect_timeout_seconds() != null) {
            out.put("connect_timeout_seconds", settings.connect_timeout_seconds());
        }
        if (settings.idle_keep_alive_seconds() != null) {
            out.put("idle_keep_alive_seconds", settings.idle_keep_alive_seconds());
        }
        if (settings.max_connections() != null) {
            out.put("max_connections", settings.max_connections());
        }
        return ordered(out);
    }

    private static Map<String, Object> toolsList() {
        List<Map<String, Object>> tools = new ArrayList<>();
        tools.add(tool("normalize-identity", "Return the canonical identity for one URL candidate.", textContextSchema(), objectSchema(
                Map.of("canonical_identity", Map.of("type", "string")), List.of("canonical_identity"))));
        tools.add(tool("parse-peer-operand", "Parse one peer operand into its role and fallback candidates.", textContextSchema(), peerSchema()));
        tools.add(tool("parse-url", "Parse one URL candidate.", textContextSchema(), urlSchema()));
        tools.sort(Comparator.comparing(tool -> (String) tool.get("name")));
        return ordered(Map.of("tools", tools));
    }

    private static Map<String, Object> tool(String name, String description, Map<String, Object> input, Map<String, Object> output) {
        return ordered(Map.of(
                "description", description,
                "inputSchema", input,
                "name", name,
                "outputSchema", output));
    }

    private static Map<String, Object> textContextSchema() {
        return objectSchema(Map.of(
                "context", objectSchema(Map.of(
                        "current_os_user", Map.of("type", "string"),
                        "current_working_directory", Map.of("type", "string")), List.of("current_os_user", "current_working_directory")),
                "text", Map.of("type", "string")), List.of("context", "text"));
    }

    private static Map<String, Object> peerSchema() {
        return objectSchema(Map.of(
                "candidates", Map.of("type", "array", "items", urlSchema()),
                "role", Map.of("type", "string")), List.of("role", "candidates"));
    }

    private static Map<String, Object> urlSchema() {
        return objectSchema(Map.ofEntries(
                Map.entry("canonical_identity", Map.of("type", "string")),
                Map.entry("endpoint_key", Map.of("type", "string")),
                Map.entry("host", Map.of("type", "string")),
                Map.entry("password", Map.of("type", "string")),
                Map.entry("path", Map.of("type", "string")),
                Map.entry("port", Map.of("type", "integer")),
                Map.entry("scheme", Map.of("type", "string")),
                Map.entry("settings", settingsSchema()),
                Map.entry("user", Map.of("type", "string"))), List.of("scheme", "canonical_identity", "settings", "path"));
    }

    private static Map<String, Object> settingsSchema() {
        return objectSchema(Map.of(
                "connect_timeout_seconds", Map.of("type", "integer"),
                "idle_keep_alive_seconds", Map.of("type", "integer"),
                "max_connections", Map.of("type", "integer")), List.of());
    }

    private static Map<String, Object> objectSchema(Map<String, Object> properties, List<String> required) {
        return ordered(Map.of(
                "additionalProperties", false,
                "properties", ordered(properties),
                "required", required,
                "type", "object"));
    }

    private static Map<String, Object> result(Object id, Object result) {
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("id", id);
        response.put("jsonrpc", "2.0");
        response.put("result", result);
        return ordered(response);
    }

    private static Map<String, Object> error(Object id, int code, String message) {
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("error", ordered(Map.of("code", code, "message", message)));
        response.put("id", id);
        response.put("jsonrpc", "2.0");
        return ordered(response);
    }

    private static Map<String, Object> toolError(Object id, String message) {
        return error(id, -32000, message);
    }

    private static Map<String, Object> ordered(Map<String, Object> input) {
        return new TreeMap<>(input);
    }

    private static void write(BufferedWriter writer, Map<String, Object> value) throws IOException {
        writer.write(Json.stringify(value));
        writer.write('\n');
        writer.flush();
    }
}
