package bounded.keyed.pool.mcp;

import bounded.keyed.pool.BoundedKeyedPool;
import bounded.keyed.pool.Handle;
import bounded.keyed.pool.PoolShutdownException;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;

public final class Main {

    // tools sorted alphabetically; all JSON object keys sorted lexicographically
    private static final String TOOLS_LIST_RESULT =
        "{\"tools\":["
        + "{\"description\":\"Acquire a resource from the pool for a key, blocking until one is available.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"key\":{\"type\":\"string\"},"
            + "\"poolId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"key\",\"poolId\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"acquire\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"},"
            + "\"resourceValue\":{\"type\":\"integer\"}"
          + "},"
          + "\"required\":[\"handleId\",\"resourceValue\"],"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Create a new bounded keyed pool with the given per-key cap and idle TTL.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"idleTtlSeconds\":{\"minimum\":0,\"type\":\"number\"},"
            + "\"maxPerKey\":{\"minimum\":1,\"type\":\"integer\"}"
          + "},"
          + "\"required\":[\"idleTtlSeconds\",\"maxPerKey\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"create-pool\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"poolId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"poolId\"],"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Discard a held resource immediately, calling destroy and freeing its slot.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"handleId\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"discard\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Return the number of times destroy has been called on resources in a pool.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"poolId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"poolId\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"get-destroy-count\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"count\":{\"type\":\"integer\"}"
          + "},"
          + "\"required\":[\"count\"],"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Return a held resource to the idle cache for its key.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"handleId\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"release\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Configure a per-key factory delay (in ms) for testing slow create behavior.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"delayMs\":{\"minimum\":0,\"type\":\"integer\"},"
            + "\"key\":{\"type\":\"string\"},"
            + "\"poolId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"delayMs\",\"key\",\"poolId\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"set-factory-delay\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Configure a per-key factory error for testing create exception propagation.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"error\":{\"type\":\"string\"},"
            + "\"key\":{\"type\":\"string\"},"
            + "\"poolId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"error\",\"key\",\"poolId\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"set-factory-error\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Shut down a pool, destroying all resources and unblocking waiting acquirers.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"poolId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"poolId\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"shutdown\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"destroyed_count\":{\"type\":\"integer\"}"
          + "},"
          + "\"required\":[\"destroyed_count\"],"
          + "\"type\":\"object\""
        + "}}"
        + "]}";

    private static final AtomicInteger poolCounter = new AtomicInteger(0);
    private static final AtomicInteger handleCounter = new AtomicInteger(0);

    private static final ConcurrentHashMap<String, ManagedPool> pools = new ConcurrentHashMap<>();
    private static final ConcurrentHashMap<String, HandleEntry> handles = new ConcurrentHashMap<>();

    private static final class ManagedPool {
        final BoundedKeyedPool<String, Integer> pool;
        final AtomicInteger resourceCounter = new AtomicInteger(0);
        final AtomicInteger destroyCount = new AtomicInteger(0);
        final ConcurrentHashMap<String, Long> factoryDelays = new ConcurrentHashMap<>();
        final ConcurrentHashMap<String, String> factoryErrors = new ConcurrentHashMap<>();

        ManagedPool(int maxPerKey, double idleTtlSeconds) {
            this.pool = new BoundedKeyedPool<>(
                key -> {
                    String err = factoryErrors.get(key);
                    if (err != null) throw new RuntimeException(err);
                    Long delayMs = factoryDelays.get(key);
                    if (delayMs != null) {
                        try { Thread.sleep(delayMs); }
                        catch (InterruptedException e) { Thread.currentThread().interrupt(); }
                    }
                    return resourceCounter.incrementAndGet();
                },
                r -> destroyCount.incrementAndGet(),
                maxPerKey,
                idleTtlSeconds
            );
        }
    }

    private static final class HandleEntry {
        final String poolId;
        final Handle<String, Integer> handle;

        HandleEntry(String poolId, Handle<String, Integer> handle) {
            this.poolId = poolId;
            this.handle = handle;
        }
    }

    public static void main(String[] args) throws IOException {
        ServerSocket server = new ServerSocket(0, 50, InetAddress.getLoopbackAddress());
        System.out.println("MCP_PORT=" + server.getLocalPort());
        System.out.flush();

        //noinspection InfiniteLoopStatement
        while (true) {
            Socket conn = server.accept();
            Thread t = new Thread(() -> handleConnection(conn));
            t.setDaemon(true);
            t.start();
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

    @SuppressWarnings("unchecked")
    private static String handleLine(String line) {
        Object id = null;
        try {
            Object parsed = JsonParser.parse(line);
            if (!(parsed instanceof Map)) return errorResponse(null, -32600, "invalid request");
            Map<String, Object> req = (Map<String, Object>) parsed;

            id = req.get("id");
            if (id == null) return null; // notification

            Object methodObj = req.get("method");
            if (!(methodObj instanceof String)) return errorResponse(id, -32600, "invalid request");
            String method = (String) methodObj;

            return switch (method) {
                case "tools/list" ->
                    "{\"id\":" + jsonId(id) + ",\"jsonrpc\":\"2.0\",\"result\":" + TOOLS_LIST_RESULT + "}";
                case "tools/call" -> handleToolsCall(id, req);
                default -> errorResponse(id, -32601, "method not found: " + method);
            };

        } catch (JsonParser.JsonException e) {
            return errorResponse(id, -32700, "parse error: " + e.getMessage());
        } catch (Exception e) {
            return errorResponse(id, -32603, "internal error: " + e.getMessage());
        }
    }

    @SuppressWarnings("unchecked")
    private static String handleToolsCall(Object id, Map<String, Object> req) {
        Object paramsObj = req.get("params");
        if (!(paramsObj instanceof Map)) return errorResponse(id, -32602, "invalid params");
        Map<String, Object> params = (Map<String, Object>) paramsObj;

        Object nameObj = params.get("name");
        if (!(nameObj instanceof String)) return errorResponse(id, -32602, "invalid params");
        String name = (String) nameObj;

        Object argsObj = params.get("arguments");
        if (!(argsObj instanceof Map)) return errorResponse(id, -32602, "invalid params");
        Map<String, Object> arguments = (Map<String, Object>) argsObj;

        try {
            return switch (name) {
                case "acquire" -> toolAcquire(id, arguments);
                case "create-pool" -> toolCreatePool(id, arguments);
                case "discard" -> toolDiscard(id, arguments);
                case "get-destroy-count" -> toolGetDestroyCount(id, arguments);
                case "release" -> toolRelease(id, arguments);
                case "set-factory-delay" -> toolSetFactoryDelay(id, arguments);
                case "set-factory-error" -> toolSetFactoryError(id, arguments);
                case "shutdown" -> toolShutdown(id, arguments);
                default -> errorResponse(id, -32000, "not implemented");
            };
        } catch (Exception e) {
            return errorResponse(id, -32603, "internal error: " + e.getMessage());
        }
    }

    private static String toolCreatePool(Object id, Map<String, Object> args) {
        Object maxObj = args.get("maxPerKey");
        Object ttlObj = args.get("idleTtlSeconds");
        if (!(maxObj instanceof Number)) return errorResponse(id, -32000, "invalid argument: maxPerKey required");
        if (!(ttlObj instanceof Number)) return errorResponse(id, -32000, "invalid argument: idleTtlSeconds required");
        int maxPerKey = ((Number) maxObj).intValue();
        double idleTtlSeconds = ((Number) ttlObj).doubleValue();
        if (maxPerKey < 1) return errorResponse(id, -32000, "invalid argument: maxPerKey must be >= 1");
        if (idleTtlSeconds < 0) return errorResponse(id, -32000, "invalid argument: idleTtlSeconds must be >= 0");

        String poolId = "p" + poolCounter.incrementAndGet();
        pools.put(poolId, new ManagedPool(maxPerKey, idleTtlSeconds));
        return successResponse(id, "{\"poolId\":" + jsonString(poolId) + "}");
    }

    private static String toolAcquire(Object id, Map<String, Object> args) {
        String poolId = requireString(id, args, "poolId");
        if (poolId == null) return errorResponse(id, -32000, "invalid argument: poolId required");
        String key = requireString(id, args, "key");
        if (key == null) return errorResponse(id, -32000, "invalid argument: key required");

        ManagedPool mp = pools.get(poolId);
        if (mp == null) return errorResponse(id, -32000, "invalid argument: unknown poolId");

        try {
            Handle<String, Integer> handle = mp.pool.acquire(key);
            String handleId = "h" + handleCounter.incrementAndGet();
            handles.put(handleId, new HandleEntry(poolId, handle));
            return successResponse(id, "{\"handleId\":" + jsonString(handleId)
                + ",\"resourceValue\":" + handle.resource() + "}");
        } catch (PoolShutdownException e) {
            return errorResponse(id, -32000, "pool is shut down");
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return errorResponse(id, -32000, "interrupted");
        }
    }

    private static String toolRelease(Object id, Map<String, Object> args) {
        String handleId = requireString(id, args, "handleId");
        if (handleId == null) return errorResponse(id, -32000, "invalid argument: handleId required");

        HandleEntry entry = handles.remove(handleId);
        if (entry == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");

        ManagedPool mp = pools.get(entry.poolId);
        if (mp == null) return errorResponse(id, -32000, "invalid argument: pool not found");
        mp.pool.release(entry.handle);
        return successResponse(id, "{}");
    }

    private static String toolDiscard(Object id, Map<String, Object> args) {
        String handleId = requireString(id, args, "handleId");
        if (handleId == null) return errorResponse(id, -32000, "invalid argument: handleId required");

        HandleEntry entry = handles.remove(handleId);
        if (entry == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");

        ManagedPool mp = pools.get(entry.poolId);
        if (mp == null) return errorResponse(id, -32000, "invalid argument: pool not found");
        mp.pool.discard(entry.handle);
        return successResponse(id, "{}");
    }

    private static String toolSetFactoryDelay(Object id, Map<String, Object> args) {
        String poolId = requireString(id, args, "poolId");
        if (poolId == null) return errorResponse(id, -32000, "invalid argument: poolId required");
        String key = requireString(id, args, "key");
        if (key == null) return errorResponse(id, -32000, "invalid argument: key required");
        Object delayObj = args.get("delayMs");
        if (!(delayObj instanceof Number)) return errorResponse(id, -32000, "invalid argument: delayMs required");
        long delayMs = ((Number) delayObj).longValue();

        ManagedPool mp = pools.get(poolId);
        if (mp == null) return errorResponse(id, -32000, "invalid argument: unknown poolId");
        mp.factoryDelays.put(key, delayMs);
        return successResponse(id, "{}");
    }

    private static String toolSetFactoryError(Object id, Map<String, Object> args) {
        String poolId = requireString(id, args, "poolId");
        if (poolId == null) return errorResponse(id, -32000, "invalid argument: poolId required");
        String key = requireString(id, args, "key");
        if (key == null) return errorResponse(id, -32000, "invalid argument: key required");
        String error = requireString(id, args, "error");
        if (error == null) return errorResponse(id, -32000, "invalid argument: error required");

        ManagedPool mp = pools.get(poolId);
        if (mp == null) return errorResponse(id, -32000, "invalid argument: unknown poolId");
        mp.factoryErrors.put(key, error);
        return successResponse(id, "{}");
    }

    private static String toolShutdown(Object id, Map<String, Object> args) {
        String poolId = requireString(id, args, "poolId");
        if (poolId == null) return errorResponse(id, -32000, "invalid argument: poolId required");

        ManagedPool mp = pools.remove(poolId);
        if (mp == null) return errorResponse(id, -32000, "invalid argument: unknown poolId");

        String pid = poolId;
        handles.entrySet().removeIf(e -> pid.equals(e.getValue().poolId));
        int beforeCount = mp.destroyCount.get();
        mp.pool.shutdown();
        int destroyed = mp.destroyCount.get() - beforeCount;
        return successResponse(id, "{\"destroyed_count\":" + destroyed + "}");
    }

    private static String toolGetDestroyCount(Object id, Map<String, Object> args) {
        String poolId = requireString(id, args, "poolId");
        if (poolId == null) return errorResponse(id, -32000, "invalid argument: poolId required");

        ManagedPool mp = pools.get(poolId);
        if (mp == null) return errorResponse(id, -32000, "invalid argument: unknown poolId");
        return successResponse(id, "{\"count\":" + mp.destroyCount.get() + "}");
    }

    private static String requireString(Object id, Map<String, Object> args, String key) {
        Object v = args.get(key);
        return v instanceof String s ? s : null;
    }

    private static String successResponse(Object id, String resultJson) {
        return "{\"id\":" + jsonId(id) + ",\"jsonrpc\":\"2.0\",\"result\":" + resultJson + "}";
    }

    private static String errorResponse(Object id, int code, String message) {
        return "{\"error\":{\"code\":" + code + ",\"message\":" + jsonString(message) + "},"
             + "\"id\":" + jsonId(id) + ",\"jsonrpc\":\"2.0\"}";
    }

    private static String jsonId(Object id) {
        if (id == null) return "null";
        if (id instanceof Long l) return Long.toString(l);
        if (id instanceof Number n) return Long.toString(n.longValue());
        if (id instanceof String s) return jsonString(s);
        return "null";
    }

    private static String jsonString(String s) {
        StringBuilder sb = new StringBuilder("\"");
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"' -> sb.append("\\\"");
                case '\\' -> sb.append("\\\\");
                case '\b' -> sb.append("\\b");
                case '\f' -> sb.append("\\f");
                case '\n' -> sb.append("\\n");
                case '\r' -> sb.append("\\r");
                case '\t' -> sb.append("\\t");
                default -> {
                    if (c < 0x20) sb.append(String.format("\\u%04x", (int) c));
                    else sb.append(c);
                }
            }
        }
        return sb.append('"').toString();
    }

    // Minimal JSON parser (no external dependencies)
    static final class JsonParser {
        private final String s;
        private int pos;

        private JsonParser(String s) { this.s = s; }

        static Object parse(String text) {
            JsonParser p = new JsonParser(text.trim());
            Object v = p.parseValue();
            p.skipWs();
            if (p.pos < p.s.length()) throw new JsonException("trailing content at " + p.pos);
            return v;
        }

        private Object parseValue() {
            skipWs();
            if (pos >= s.length()) throw new JsonException("unexpected end");
            char c = s.charAt(pos);
            if (c == '{') return parseObject();
            if (c == '[') return parseArray();
            if (c == '"') return parseString();
            if (c == 't' || c == 'f') return parseBoolean();
            if (c == 'n') { parseNull(); return null; }
            if (c == '-' || Character.isDigit(c)) return parseNumber();
            throw new JsonException("unexpected char '" + c + "' at " + pos);
        }

        Map<String, Object> parseObject() {
            pos++; // {
            Map<String, Object> m = new LinkedHashMap<>();
            skipWs();
            if (pos < s.length() && s.charAt(pos) == '}') { pos++; return m; }
            while (true) {
                skipWs();
                if (s.charAt(pos) != '"') throw new JsonException("expected string key at " + pos);
                String key = parseString();
                skipWs();
                if (s.charAt(pos) != ':') throw new JsonException("expected ':' at " + pos);
                pos++;
                Object val = parseValue();
                m.put(key, val);
                skipWs();
                char sep = s.charAt(pos);
                if (sep == '}') { pos++; return m; }
                if (sep == ',') { pos++; }
                else throw new JsonException("expected '}' or ',' at " + pos);
            }
        }

        private List<Object> parseArray() {
            pos++; // [
            List<Object> list = new ArrayList<>();
            skipWs();
            if (pos < s.length() && s.charAt(pos) == ']') { pos++; return list; }
            while (true) {
                list.add(parseValue());
                skipWs();
                char sep = s.charAt(pos);
                if (sep == ']') { pos++; return list; }
                if (sep == ',') { pos++; }
                else throw new JsonException("expected ']' or ',' at " + pos);
            }
        }

        private String parseString() {
            pos++; // "
            StringBuilder sb = new StringBuilder();
            while (pos < s.length()) {
                char c = s.charAt(pos++);
                if (c == '"') return sb.toString();
                if (c == '\\') {
                    char esc = s.charAt(pos++);
                    switch (esc) {
                        case '"' -> sb.append('"');
                        case '\\' -> sb.append('\\');
                        case '/' -> sb.append('/');
                        case 'b' -> sb.append('\b');
                        case 'f' -> sb.append('\f');
                        case 'n' -> sb.append('\n');
                        case 'r' -> sb.append('\r');
                        case 't' -> sb.append('\t');
                        case 'u' -> {
                            sb.append((char) Integer.parseInt(s.substring(pos, pos + 4), 16));
                            pos += 4;
                        }
                        default -> throw new JsonException("invalid escape \\" + esc);
                    }
                } else {
                    sb.append(c);
                }
            }
            throw new JsonException("unterminated string");
        }

        private Number parseNumber() {
            int start = pos;
            if (s.charAt(pos) == '-') pos++;
            while (pos < s.length() && Character.isDigit(s.charAt(pos))) pos++;
            boolean isFloat = false;
            if (pos < s.length() && s.charAt(pos) == '.') {
                isFloat = true; pos++;
                while (pos < s.length() && Character.isDigit(s.charAt(pos))) pos++;
            }
            if (pos < s.length() && (s.charAt(pos) == 'e' || s.charAt(pos) == 'E')) {
                isFloat = true; pos++;
                if (pos < s.length() && (s.charAt(pos) == '+' || s.charAt(pos) == '-')) pos++;
                while (pos < s.length() && Character.isDigit(s.charAt(pos))) pos++;
            }
            String num = s.substring(start, pos);
            return isFloat ? Double.parseDouble(num) : Long.parseLong(num);
        }

        private boolean parseBoolean() {
            if (s.startsWith("true", pos)) { pos += 4; return true; }
            if (s.startsWith("false", pos)) { pos += 5; return false; }
            throw new JsonException("invalid boolean at " + pos);
        }

        private void parseNull() {
            if (s.startsWith("null", pos)) { pos += 4; return; }
            throw new JsonException("invalid null at " + pos);
        }

        private void skipWs() {
            while (pos < s.length() && s.charAt(pos) <= ' ') pos++;
        }

        static final class JsonException extends RuntimeException {
            JsonException(String msg) { super(msg); }
        }
    }
}
