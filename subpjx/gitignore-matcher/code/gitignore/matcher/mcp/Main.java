package gitignore.matcher.mcp;

import gitignore.matcher.GitignoreMatcher;
import gitignore.matcher.MatchResult;
import gitignore.matcher.Patterns;
import gitignore.matcher.StackEntry;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.PrintWriter;
import java.net.ServerSocket;
import java.net.Socket;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class Main {

    private static final String TOOLS_LIST_RESULT =
        "{\"tools\":[" +
        "{\"description\":\"Parse gitignore-syntax pattern text into a reusable pattern set.\"," +
        "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{\"text\":{\"type\":\"string\"}}," +
        "\"required\":[\"text\"],\"type\":\"object\"}," +
        "\"name\":\"compile\"," +
        "\"outputSchema\":{\"additionalProperties\":false,\"properties\":{\"patterns\":{\"type\":\"string\"}}," +
        "\"required\":[\"patterns\"],\"type\":\"object\"}}," +
        "{\"description\":\"Decide whether a relative path is ignored given a stack of gitignore pattern sets.\"," +
        "\"inputSchema\":{\"additionalProperties\":false,\"properties\":{" +
        "\"is_directory\":{\"type\":\"boolean\"}," +
        "\"relative_path\":{\"type\":\"string\"}," +
        "\"stack\":{\"items\":{\"additionalProperties\":false,\"properties\":{" +
        "\"patterns\":{\"type\":\"string\"}," +
        "\"scope\":{\"type\":\"string\"}}," +
        "\"required\":[\"scope\",\"patterns\"],\"type\":\"object\"},\"type\":\"array\"}}," +
        "\"required\":[\"is_directory\",\"relative_path\",\"stack\"],\"type\":\"object\"}," +
        "\"name\":\"match\"," +
        "\"outputSchema\":{\"additionalProperties\":false,\"properties\":{\"result\":{\"type\":\"string\"}}," +
        "\"required\":[\"result\"],\"type\":\"object\"}}" +
        "]}";

    public static void main(String[] args) throws IOException {
        try (ServerSocket server = new ServerSocket(0, 1, java.net.InetAddress.getByName("127.0.0.1"))) {
            System.out.println("MCP_PORT=" + server.getLocalPort());
            System.out.flush();
            while (true) {
                Socket client = server.accept();
                handle(client);
            }
        }
    }

    private static void handle(Socket client) {
        try (client;
             BufferedReader in = new BufferedReader(new InputStreamReader(client.getInputStream(), java.nio.charset.StandardCharsets.UTF_8));
             PrintWriter out = new PrintWriter(new java.io.OutputStreamWriter(client.getOutputStream(), java.nio.charset.StandardCharsets.UTF_8), true)) {
            String line;
            while ((line = in.readLine()) != null) {
                String response = processLine(line.trim());
                if (response != null) out.println(response);
            }
        } catch (IOException e) {
            System.err.println("client error: " + e.getMessage());
        }
    }

    private static String processLine(String line) {
        if (line.isEmpty()) return null;
        Map<String, Object> req;
        try {
            req = JsonParser.parseObject(line);
        } catch (Exception e) {
            return errorResponse(null, -32700, "parse error");
        }

        Object id = req.get("id");
        String method = asString(req.get("method"));
        if (method == null) return errorResponse(id, -32600, "invalid request");

        return switch (method) {
            case "tools/list" -> successResponse(id, TOOLS_LIST_RESULT);
            case "tools/call" -> handleToolsCall(id, req);
            default -> errorResponse(id, -32601, "method not found: " + method);
        };
    }

    @SuppressWarnings("unchecked")
    private static String handleToolsCall(Object id, Map<String, Object> req) {
        Object paramsObj = req.get("params");
        if (!(paramsObj instanceof Map)) return errorResponse(id, -32602, "invalid params");
        Map<String, Object> params = (Map<String, Object>) paramsObj;
        String name = asString(params.get("name"));
        if (name == null) return errorResponse(id, -32602, "invalid params: name required");
        Object argsObj = params.get("arguments");
        if (!(argsObj instanceof Map)) return errorResponse(id, -32602, "invalid params: arguments required");
        Map<String, Object> args = (Map<String, Object>) argsObj;

        return switch (name) {
            case "compile" -> toolCompile(id, args);
            case "match" -> toolMatch(id, args);
            default -> errorResponse(id, -32000, "not implemented");
        };
    }

    private static String toolCompile(Object id, Map<String, Object> args) {
        Object textObj = args.get("text");
        if (!(textObj instanceof String)) return errorResponse(id, -32000, "invalid argument: text is required");
        GitignoreMatcher.compile((String) textObj);
        return successResponse(id, "{\"patterns\":" + jsonString((String) textObj) + "}");
    }

    @SuppressWarnings("unchecked")
    private static String toolMatch(Object id, Map<String, Object> args) {
        Object stackObj = args.get("stack");
        Object relPathObj = args.get("relative_path");
        Object isDirObj = args.get("is_directory");

        if (!(stackObj instanceof List)) return errorResponse(id, -32000, "invalid argument: stack is required");
        if (!(relPathObj instanceof String)) return errorResponse(id, -32000, "invalid argument: relative_path is required");
        if (!(isDirObj instanceof Boolean)) return errorResponse(id, -32000, "invalid argument: is_directory is required");

        List<Object> stackList = (List<Object>) stackObj;
        List<StackEntry> stack = new ArrayList<>();
        for (Object entryObj : stackList) {
            if (!(entryObj instanceof Map)) return errorResponse(id, -32000, "invalid argument: stack entry must be object");
            Map<String, Object> entry = (Map<String, Object>) entryObj;
            Object scopeObj = entry.get("scope");
            Object patternsObj = entry.get("patterns");
            if (!(scopeObj instanceof String)) return errorResponse(id, -32000, "invalid argument: stack entry scope required");
            if (!(patternsObj instanceof String)) return errorResponse(id, -32000, "invalid argument: stack entry patterns required");
            Patterns patterns = GitignoreMatcher.compile((String) patternsObj);
            stack.add(new StackEntry((String) scopeObj, patterns));
        }

        MatchResult result = GitignoreMatcher.match(stack, (String) relPathObj, (Boolean) isDirObj);
        String resultStr = result == MatchResult.IGNORED ? "Ignored" : "NotIgnored";
        return successResponse(id, "{\"result\":" + jsonString(resultStr) + "}");
    }

    // --- response builders ---

    private static String successResponse(Object id, String resultJson) {
        return "{\"id\":" + jsonId(id) + ",\"jsonrpc\":\"2.0\",\"result\":" + resultJson + "}";
    }

    private static String errorResponse(Object id, int code, String message) {
        return "{\"error\":{\"code\":" + code + ",\"message\":" + jsonString(message) + "}," +
               "\"id\":" + jsonId(id) + ",\"jsonrpc\":\"2.0\"}";
    }

    private static String jsonId(Object id) {
        if (id == null) return "null";
        if (id instanceof Long) return Long.toString((Long) id);
        if (id instanceof String) return jsonString((String) id);
        return "null";
    }

    private static String jsonString(String s) {
        StringBuilder sb = new StringBuilder("\"");
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"' -> sb.append("\\\"");
                case '\\' -> sb.append("\\\\");
                case '\n' -> sb.append("\\n");
                case '\r' -> sb.append("\\r");
                case '\t' -> sb.append("\\t");
                default -> {
                    if (c < 32) sb.append(String.format("\\u%04x", (int) c));
                    else sb.append(c);
                }
            }
        }
        return sb.append('"').toString();
    }

    private static String asString(Object o) {
        return o instanceof String s ? s : null;
    }

    // --- minimal JSON parser ---

    static final class JsonParser {
        private final String input;
        private int pos;

        private JsonParser(String input) {
            this.input = input;
        }

        @SuppressWarnings("unchecked")
        static Map<String, Object> parseObject(String json) {
            JsonParser p = new JsonParser(json);
            p.skipWs();
            Object val = p.parseValue();
            if (!(val instanceof Map)) throw new IllegalArgumentException("expected JSON object");
            return (Map<String, Object>) val;
        }

        private Object parseValue() {
            skipWs();
            if (pos >= input.length()) throw new IllegalArgumentException("unexpected end");
            char c = input.charAt(pos);
            if (c == '{') return parseObj();
            if (c == '[') return parseArr();
            if (c == '"') return parseStr();
            if (c == 't') { expect("true"); return Boolean.TRUE; }
            if (c == 'f') { expect("false"); return Boolean.FALSE; }
            if (c == 'n') { expect("null"); return null; }
            if (c == '-' || Character.isDigit(c)) return parseNum();
            throw new IllegalArgumentException("unexpected char: " + c + " at " + pos);
        }

        private Map<String, Object> parseObj() {
            consume('{');
            Map<String, Object> map = new LinkedHashMap<>();
            skipWs();
            if (peek() == '}') { pos++; return map; }
            while (true) {
                skipWs();
                String key = parseStr();
                skipWs();
                consume(':');
                skipWs();
                map.put(key, parseValue());
                skipWs();
                char sep = input.charAt(pos++);
                if (sep == '}') break;
                if (sep != ',') throw new IllegalArgumentException("expected , or }");
            }
            return map;
        }

        private List<Object> parseArr() {
            consume('[');
            List<Object> list = new ArrayList<>();
            skipWs();
            if (peek() == ']') { pos++; return list; }
            while (true) {
                skipWs();
                list.add(parseValue());
                skipWs();
                char sep = input.charAt(pos++);
                if (sep == ']') break;
                if (sep != ',') throw new IllegalArgumentException("expected , or ]");
            }
            return list;
        }

        private String parseStr() {
            consume('"');
            StringBuilder sb = new StringBuilder();
            while (pos < input.length()) {
                char c = input.charAt(pos++);
                if (c == '"') return sb.toString();
                if (c == '\\') {
                    char esc = input.charAt(pos++);
                    switch (esc) {
                        case '"' -> sb.append('"');
                        case '\\' -> sb.append('\\');
                        case '/' -> sb.append('/');
                        case 'n' -> sb.append('\n');
                        case 'r' -> sb.append('\r');
                        case 't' -> sb.append('\t');
                        case 'b' -> sb.append('\b');
                        case 'f' -> sb.append('\f');
                        case 'u' -> {
                            sb.append((char) Integer.parseInt(input.substring(pos, pos + 4), 16));
                            pos += 4;
                        }
                        default -> sb.append(esc);
                    }
                } else {
                    sb.append(c);
                }
            }
            throw new IllegalArgumentException("unterminated string");
        }

        private Number parseNum() {
            int start = pos;
            if (pos < input.length() && input.charAt(pos) == '-') pos++;
            while (pos < input.length() && Character.isDigit(input.charAt(pos))) pos++;
            boolean decimal = false;
            if (pos < input.length() && input.charAt(pos) == '.') {
                decimal = true;
                pos++;
                while (pos < input.length() && Character.isDigit(input.charAt(pos))) pos++;
            }
            if (pos < input.length() && (input.charAt(pos) == 'e' || input.charAt(pos) == 'E')) {
                decimal = true;
                pos++;
                if (pos < input.length() && (input.charAt(pos) == '+' || input.charAt(pos) == '-')) pos++;
                while (pos < input.length() && Character.isDigit(input.charAt(pos))) pos++;
            }
            String numStr = input.substring(start, pos);
            return decimal ? Double.parseDouble(numStr) : Long.parseLong(numStr);
        }

        private void skipWs() {
            while (pos < input.length() && Character.isWhitespace(input.charAt(pos))) pos++;
        }

        private void consume(char c) {
            if (pos >= input.length() || input.charAt(pos) != c)
                throw new IllegalArgumentException("expected '" + c + "' at " + pos);
            pos++;
        }

        private void expect(String s) {
            if (!input.startsWith(s, pos)) throw new IllegalArgumentException("expected " + s + " at " + pos);
            pos += s.length();
        }

        private char peek() {
            if (pos >= input.length()) throw new IllegalArgumentException("unexpected end");
            return input.charAt(pos);
        }
    }
}
