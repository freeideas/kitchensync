package url.normalizer.mcp;

import url.normalizer.NormalizedUrl;
import url.normalizer.ParseContext;
import url.normalizer.UrlNormalizer;
import url.normalizer.UrlNormalizerError;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.SocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

public final class Main {
    private static final String TOOL_NAME = "normalize-url";
    private static final ToolDefinition NORMALIZE_TOOL = createNormalizeTool();
    private static final String TOOLS_LIST_JSON;

    static {
        List<ToolDefinition> tools = new ArrayList<>();
        tools.add(NORMALIZE_TOOL);
        tools.sort(Comparator.comparing(tool -> tool.name));
        TOOLS_LIST_JSON = JsonCodec.stringify(Map.of("tools", tools));
    }

    public static void main(String[] args) throws Exception {
        try (ServerSocket serverSocket = new ServerSocket()) {
            SocketAddress address = new java.net.InetSocketAddress("127.0.0.1", 0);
            serverSocket.bind(address);
            System.out.println("MCP_PORT=" + serverSocket.getLocalPort());

            while (true) {
                try (Socket socket = serverSocket.accept();
                     BufferedReader reader = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
                     BufferedWriter writer = new BufferedWriter(new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8))) {

                    String line;
                    while ((line = reader.readLine()) != null) {
                        Response response = handleRequest(line);
                        if (response == null || !response.hasId) {
                            continue;
                        }
                        writer.write(response.body);
                        writer.newLine();
                        writer.flush();
                    }
                }
            }
        }
    }

    private static Response handleRequest(String line) {
        Object request;
        try {
            request = JsonCodec.parse(line);
        } catch (Exception e) {
            return Response.error(null, -32700, "parse error");
        }

        if (!(request instanceof Map<?, ?> requestMap)) {
            return Response.error(null, -32600, "invalid request");
        }

        if (!requestMap.containsKey("id")) {
            return null;
        }

        Object id = requestMap.get("id");
        if (!"2.0".equals(requestMap.get("jsonrpc"))) {
            return Response.error(id, -32600, "invalid request");
        }

        if (!(requestMap.get("method") instanceof String method)) {
            return Response.error(id, -32600, "invalid request");
        }

        if ("tools/list".equals(method)) {
            return Response.result(id, JsonCodec.parse(TOOLS_LIST_JSON));
        }
        if ("tools/call".equals(method)) {
            return handleToolsCall(id, requestMap.get("params"));
        }
        return Response.error(id, -32601, "method not found: " + method);
    }

    private static Response handleToolsCall(Object id, Object paramsObject) {
        if (!(paramsObject instanceof Map<?, ?> params)) {
            return Response.error(id, -32602, "invalid params");
        }

        Object toolNameValue = params.get("name");
        if (!(toolNameValue instanceof String name)) {
            return Response.error(id, -32000, "invalid argument: name is required");
        }

        if (!TOOL_NAME.equals(name)) {
            return Response.error(id, -32602, "invalid params");
        }

        if (!(params.get("arguments") instanceof Map<?, ?> arguments)) {
            return Response.error(id, -32000, "invalid argument: arguments is required");
        }

        try {
            String text = requireString(arguments, "text");
            ParseContext context = requireContext(arguments);
            NormalizedUrl normalizedUrl = UrlNormalizer.normalize_url(text, context);
            return Response.result(id, Map.of("normalized_url", normalizedUrl.normalized_url()));
        } catch (ValidationError e) {
            return Response.error(id, -32000, "invalid argument: " + e.getMessage());
        } catch (UrlNormalizerError e) {
            return Response.error(id, -32000, e.code());
        } catch (Exception e) {
            return Response.error(id, -32603, "internal error");
        }
    }

    private static ParseContext requireContext(Map<?, ?> arguments) throws ValidationError {
        Object contextObject = arguments.get("context");
        if (!(contextObject instanceof Map<?, ?> context)) {
            throw new ValidationError("context is required");
        }

        String cwd = requireString(context, "current_working_directory", "context.current_working_directory");
        String user = requireString(context, "current_os_user", "context.current_os_user");
        return new ParseContext(cwd, user);
    }

    private static String requireString(Map<?, ?> map, String key) throws ValidationError {
        if (!map.containsKey(key)) {
            throw new ValidationError(key + " is required");
        }
        Object value = map.get(key);
        if (!(value instanceof String text)) {
            throw new ValidationError(key + " must be a string");
        }
        return text;
    }

    private static String requireString(Map<?, ?> map, String key, String path) throws ValidationError {
        if (!map.containsKey(key)) {
            throw new ValidationError(path + " is required");
        }
        Object value = map.get(key);
        if (!(value instanceof String text)) {
            throw new ValidationError(path + " must be a string");
        }
        return text;
    }

    private static ToolDefinition createNormalizeTool() {
        Map<String, Object> contextInputSchema = new LinkedHashMap<>();
        contextInputSchema.put("type", "object");
        contextInputSchema.put("properties", new LinkedHashMap<>() {{
            put("current_working_directory", Map.of("type", "string"));
            put("current_os_user", Map.of("type", "string"));
        }});
        contextInputSchema.put("required", List.of("current_working_directory", "current_os_user"));
        contextInputSchema.put("additionalProperties", false);

        Map<String, Object> input = new LinkedHashMap<>();
        input.put("type", "object");
        input.put("properties", new LinkedHashMap<>() {{
            put("text", Map.of("type", "string"));
            put("context", contextInputSchema);
        }});
        input.put("required", List.of("text", "context"));
        input.put("additionalProperties", false);

        Map<String, Object> output = new LinkedHashMap<>();
        output.put("type", "object");
        output.put("properties", Map.of("normalized_url", Map.of("type", "string")));
        output.put("required", List.of("normalized_url"));
        output.put("additionalProperties", false);

        return new ToolDefinition(
                TOOL_NAME,
                "Normalize a URL text into a canonical URL.",
                input,
                output
        );
    }

    private static final class ToolDefinition {
        final String name;
        final String description;
        final Map<String, Object> inputSchema;
        final Map<String, Object> outputSchema;

        ToolDefinition(String name, String description, Map<String, Object> inputSchema, Map<String, Object> outputSchema) {
            this.name = name;
            this.description = description;
            this.inputSchema = inputSchema;
            this.outputSchema = outputSchema;
        }

        Map<String, Object> asMap() {
            Map<String, Object> map = new LinkedHashMap<>();
            map.put("name", name);
            map.put("description", description);
            map.put("inputSchema", inputSchema);
            map.put("outputSchema", outputSchema);
            return map;
        }
    }

    private static final class Response {
        final boolean hasId;
        final String body;

        Response(boolean hasId, String body) {
            this.hasId = hasId;
            this.body = body;
        }

        static Response result(Object id, Object result) {
            Map<String, Object> envelope = new LinkedHashMap<>();
            envelope.put("jsonrpc", "2.0");
            envelope.put("id", id);
            envelope.put("result", result);
            return new Response(true, JsonCodec.stringify(envelope));
        }

        static Response error(Object id, int code, String message) {
            Map<String, Object> error = new LinkedHashMap<>();
            error.put("code", code);
            error.put("message", message);
            Map<String, Object> envelope = new LinkedHashMap<>();
            envelope.put("jsonrpc", "2.0");
            envelope.put("id", id);
            envelope.put("error", error);
            return new Response(true, JsonCodec.stringify(envelope));
        }
    }

    private static final class ValidationError extends Exception {
        ValidationError(String message) {
            super(message);
        }
    }

    private static final class JsonCodec {
        static Object parse(String json) {
            return new Parser(json).parse();
        }

        static String stringify(Object value) {
            return new Serializer().stringify(value);
        }

        private static final class Serializer {
            String stringify(Object value) {
                if (value == null) {
                    return "null";
                }
                if (value instanceof String s) {
                    return quote(s);
                }
                if (value instanceof Number n) {
                    return n.toString();
                }
                if (value instanceof Boolean b) {
                    return b ? "true" : "false";
                }
                if (value instanceof Map<?, ?> map) {
                    List<String> keys = new ArrayList<>(map.keySet().size());
                    for (Object key : map.keySet()) {
                        keys.add(String.valueOf(key));
                    }
                    keys.sort(Comparator.naturalOrder());

                    StringBuilder builder = new StringBuilder("{");
                    for (int i = 0; i < keys.size(); i++) {
                        if (i > 0) {
                            builder.append(",");
                        }
                        String key = keys.get(i);
                        builder.append(quote(key)).append(":").append(stringify(map.get(key)));
                    }
                    builder.append("}");
                    return builder.toString();
                }
                if (value instanceof Iterable<?> iterable) {
                    StringBuilder builder = new StringBuilder("[");
                    boolean first = true;
                    for (Object item : iterable) {
                        if (!first) {
                            builder.append(",");
                        }
                        first = false;
                        if (item instanceof ToolDefinition tool) {
                            builder.append(stringify(tool.asMap()));
                        } else {
                            builder.append(stringify(item));
                        }
                    }
                    builder.append("]");
                    return builder.toString();
                }
                if (value instanceof ToolDefinition toolDefinition) {
                    return stringify(toolDefinition.asMap());
                }
                return quote(String.valueOf(value));
            }

            private String quote(String raw) {
                StringBuilder escaped = new StringBuilder("\"");
                for (int i = 0; i < raw.length(); i++) {
                    char c = raw.charAt(i);
                    switch (c) {
                        case '"':
                            escaped.append("\\\"");
                            break;
                        case '\\':
                            escaped.append("\\\\");
                            break;
                        case '\b':
                            escaped.append("\\b");
                            break;
                        case '\f':
                            escaped.append("\\f");
                            break;
                        case '\n':
                            escaped.append("\\n");
                            break;
                        case '\r':
                            escaped.append("\\r");
                            break;
                        case '\t':
                            escaped.append("\\t");
                            break;
                        default:
                            if (c < 0x20) {
                                escaped.append(String.format(Locale.ROOT, "\\u%04x", (int) c));
                            } else {
                                escaped.append(c);
                            }
                    }
                }
                escaped.append("\"");
                return escaped.toString();
            }
        }

        private static final class Parser {
            private final String raw;
            private int index;

            Parser(String raw) {
                this.raw = raw;
            }

            Object parse() {
                skipWhitespace();
                Object value = parseValue();
                skipWhitespace();
                if (index != raw.length()) {
                    throw new IllegalArgumentException("trailing data");
                }
                return value;
            }

            private Object parseValue() {
                if (index >= raw.length()) {
                    throw new IllegalArgumentException("eof");
                }
                char c = raw.charAt(index);
                return switch (c) {
                    case '"' -> parseString();
                    case '{' -> parseObject();
                    case '[' -> parseArray();
                    case 't' -> parseLiteral("true", true);
                    case 'f' -> parseLiteral("false", false);
                    case 'n' -> parseLiteral("null", null);
                    default -> {
                        if (c == '-' || Character.isDigit(c)) {
                            yield parseNumber();
                        }
                        throw new IllegalArgumentException("value");
                    }
                };
            }

            private Object parseLiteral(String expected, Object value) {
                if (!raw.regionMatches(index, expected, 0, expected.length())) {
                    throw new IllegalArgumentException("literal");
                }
                index += expected.length();
                return value;
            }

            private Map<String, Object> parseObject() {
                Map<String, Object> object = new LinkedHashMap<>();
                consume('{');
                skipWhitespace();
                if (consumeIf('}')) {
                    return object;
                }
                while (true) {
                    skipWhitespace();
                    String key = parseString();
                    skipWhitespace();
                    consume(':');
                    skipWhitespace();
                    Object value = parseValue();
                    object.put(key, value);
                    skipWhitespace();
                    if (consumeIf(',')) {
                        continue;
                    }
                    consume('}');
                    break;
                }
                return object;
            }

            private List<Object> parseArray() {
                List<Object> array = new ArrayList<>();
                consume('[');
                skipWhitespace();
                if (consumeIf(']')) {
                    return array;
                }
                while (true) {
                    skipWhitespace();
                    array.add(parseValue());
                    skipWhitespace();
                    if (consumeIf(',')) {
                        continue;
                    }
                    consume(']');
                    break;
                }
                return array;
            }

            private Number parseNumber() {
                int start = index;
                if (raw.charAt(index) == '-') {
                    index++;
                }
                if (index >= raw.length() || !Character.isDigit(raw.charAt(index))) {
                    throw new IllegalArgumentException("number");
                }
                while (index < raw.length() && Character.isDigit(raw.charAt(index))) {
                    index++;
                }
                if (index < raw.length() && raw.charAt(index) == '.') {
                    throw new IllegalArgumentException("number");
                }
                return Long.parseLong(raw.substring(start, index));
            }

            private String parseString() {
                consume('"');
                StringBuilder value = new StringBuilder();
                while (index < raw.length()) {
                    char c = raw.charAt(index++);
                    if (c == '"') {
                        return value.toString();
                    }
                    if (c == '\\') {
                        if (index >= raw.length()) {
                            throw new IllegalArgumentException("string");
                        }
                        char escaped = raw.charAt(index++);
                        switch (escaped) {
                            case '"':
                                value.append('"');
                                break;
                            case '\\':
                                value.append('\\');
                                break;
                            case '/':
                                value.append('/');
                                break;
                            case 'b':
                                value.append('\b');
                                break;
                            case 'f':
                                value.append('\f');
                                break;
                            case 'n':
                                value.append('\n');
                                break;
                            case 'r':
                                value.append('\r');
                                break;
                            case 't':
                                value.append('\t');
                                break;
                            case 'u':
                                if (index + 4 > raw.length()) {
                                    throw new IllegalArgumentException("string");
                                }
                                String hex = raw.substring(index, index + 4);
                                index += 4;
                                value.append((char) Integer.parseInt(hex, 16));
                                break;
                            default:
                                throw new IllegalArgumentException("string");
                        }
                        continue;
                    }
                    value.append(c);
                }
                throw new IllegalArgumentException("string");
            }

            private void skipWhitespace() {
                while (index < raw.length()) {
                    char c = raw.charAt(index);
                    if (c != ' ' && c != '\n' && c != '\r' && c != '\t') {
                        break;
                    }
                    index++;
                }
            }

            private boolean consumeIf(char expected) {
                if (index < raw.length() && raw.charAt(index) == expected) {
                    index++;
                    return true;
                }
                return false;
            }

            private void consume(char expected) {
                if (!consumeIf(expected)) {
                    throw new IllegalArgumentException("consume");
                }
            }
        }
    }
}

