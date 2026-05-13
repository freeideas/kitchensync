package url.parser.mcp;

import url.parser.*;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

public final class Main {

    // tools/list result — keys sorted lexicographically, tools sorted by name
    private static final String TOOLS_LIST_JSON =
        "{\"tools\":["
        + "{\"description\":\"Return the canonical identity string for a single URL expression.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"cwd\":{\"type\":\"string\"},"
            + "\"default_user\":{\"type\":\"string\"},"
            + "\"url\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"cwd\",\"default_user\",\"url\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"normalize\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"identity\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"identity\"],"
          + "\"type\":\"object\""
        + "}"
        + "},"
        + "{\"description\":\"Parse a tagged URL group expression into a structured TaggedGroup.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"cwd\":{\"type\":\"string\"},"
            + "\"default_user\":{\"type\":\"string\"},"
            + "\"text\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"cwd\",\"default_user\",\"text\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"parse\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"role\":{\"type\":\"string\"},"
            + "\"urls\":{"
              + "\"items\":{"
                + "\"additionalProperties\":false,"
                + "\"properties\":{"
                  + "\"host\":{\"type\":\"string\"},"
                  + "\"identity\":{\"type\":\"string\"},"
                  + "\"password\":{\"type\":\"string\"},"
                  + "\"path\":{\"type\":\"string\"},"
                  + "\"port\":{\"type\":\"integer\"},"
                  + "\"query\":{\"additionalProperties\":{\"type\":\"string\"},\"type\":\"object\"},"
                  + "\"scheme\":{\"type\":\"string\"},"
                  + "\"user\":{\"type\":\"string\"}"
                + "},"
                + "\"required\":[\"identity\",\"path\",\"query\",\"scheme\"],"
                + "\"type\":\"object\""
              + "},"
              + "\"type\":\"array\""
            + "}"
          + "},"
          + "\"required\":[\"role\",\"urls\"],"
          + "\"type\":\"object\""
        + "}"
        + "}"
        + "]}";

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
                out.println(response);
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
            if (id == null) return null; // notification, ignore

            Object methodObj = req.get("method");
            if (!(methodObj instanceof String)) return errorResponse(id, -32600, "invalid request");
            String method = (String) methodObj;

            if ("tools/list".equals(method)) {
                return "{\"jsonrpc\":\"2.0\",\"id\":" + Json.write(id)
                        + ",\"result\":" + TOOLS_LIST_JSON + "}";
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
            return switch (name) {
                case "normalize" -> toolNormalize(id, args);
                case "parse" -> toolParse(id, args);
                default -> errorResponse(id, -32000, "not implemented");
            };
        } catch (ParseException e) {
            return errorResponse(id, -32000, "invalid argument: " + e.getMessage());
        } catch (Exception e) {
            return errorResponse(id, -32603, "internal error: " + e.getMessage());
        }
    }

    private static String toolNormalize(Object id, Map<String, Object> args) {
        String url = requireString(args, "url");
        String cwd = requireString(args, "cwd");
        String defaultUser = requireString(args, "default_user");
        String identity = UrlParser.normalize(url, cwd, defaultUser);
        return successResponse(id, identity);
    }

    private static String toolParse(Object id, Map<String, Object> args) {
        String text = requireString(args, "text");
        String cwd = requireString(args, "cwd");
        String defaultUser = requireString(args, "default_user");
        TaggedGroup group = UrlParser.parse(text, cwd, defaultUser);
        return successResponse(id, serializeTaggedGroup(group));
    }

    private static Map<String, Object> serializeTaggedGroup(TaggedGroup group) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("role", roleName(group.role()));
        List<Object> urls = new ArrayList<>();
        for (ParsedUrl u : group.urls()) urls.add(serializeParsedUrl(u));
        m.put("urls", urls);
        return m;
    }

    private static Map<String, Object> serializeParsedUrl(ParsedUrl u) {
        Map<String, Object> m = new LinkedHashMap<>();
        if (u.host() != null) m.put("host", u.host());
        m.put("identity", u.identity());
        if (u.password() != null) m.put("password", u.password());
        m.put("path", u.path());
        if (u.port() != null) m.put("port", u.port());
        m.put("query", u.query());
        m.put("scheme", u.scheme());
        if (u.user() != null) m.put("user", u.user());
        return m;
    }

    private static String roleName(Role role) {
        return switch (role) {
            case NORMAL -> "Normal";
            case CANON -> "Canon";
            case SUBORDINATE -> "Subordinate";
        };
    }

    private static String requireString(Map<String, Object> args, String key) {
        Object v = args.get(key);
        if (!(v instanceof String))
            throw new ParseException("missing or invalid argument: " + key);
        return (String) v;
    }

    private static String successResponse(Object id, Object result) {
        String resultText = Json.write(result);
        return "{\"id\":" + Json.write(id)
                + ",\"jsonrpc\":\"2.0\",\"result\":{\"content\":[{\"text\":"
                + Json.write(resultText) + ",\"type\":\"text\"}]}}";
    }

    private static String errorResponse(Object id, int code, String message) {
        String idPart = (id != null) ? Json.write(id) : "null";
        return "{\"error\":{\"code\":" + code + ",\"message\":"
                + Json.write(message) + "},\"id\":" + idPart + ",\"jsonrpc\":\"2.0\"}";
    }
}
