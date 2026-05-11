package connection.pool.mcp;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.function.Consumer;
import java.util.function.Supplier;

import connection.pool.Pool;
import connection.pool.PoolSettings;
import connection.pool.Pools;

final class Dispatcher {

    private final Pools registry = new Pools();
    private final Map<String, PoolInfo> pools = new ConcurrentHashMap<>();

    private static final class PoolInfo {
        final Object keyValue;
        final long openDelayMs;
        final AtomicInteger failsRemaining;
        final AtomicInteger openCount = new AtomicInteger(0);
        final AtomicInteger closeCount = new AtomicInteger(0);
        final AtomicInteger connSeq = new AtomicInteger(0);
        final boolean hasOnEvent;
        final List<Map<String, Object>> events = Collections.synchronizedList(new ArrayList<>());
        Pool<String> pool;

        PoolInfo(Object keyValue, boolean hasOnEvent, long openDelayMs, int failCount) {
            this.keyValue = keyValue;
            this.hasOnEvent = hasOnEvent;
            this.openDelayMs = openDelayMs;
            this.failsRemaining = new AtomicInteger(failCount);
        }

        String doOpen(String poolId) {
            int prevFails = failsRemaining.getAndDecrement();
            if (prevFails > 0) {
                throw new RuntimeException("open failed (configured)");
            }
            if (openDelayMs > 0) {
                try {
                    Thread.sleep(openDelayMs);
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    throw new RuntimeException("open interrupted");
                }
            }
            int n = connSeq.incrementAndGet();
            openCount.incrementAndGet();
            return "conn-" + poolId + "-" + n;
        }

        void doClose(String conn) {
            closeCount.incrementAndGet();
        }

        void recordEvent(String kind, Object key, int inUse, int mc) {
            Map<String, Object> evt = new LinkedHashMap<>();
            evt.put("kind", kind);
            evt.put("key", key);
            evt.put("in_use", inUse);
            evt.put("mc", mc);
            events.add(evt);
        }
    }

    String dispatch(String line) {
        Object req;
        try {
            req = Json.parse(line);
        } catch (Exception e) {
            return error(null, -32700, "parse error: " + e.getMessage());
        }
        if (!(req instanceof Map)) {
            return error(null, -32600, "invalid request");
        }
        Map<?, ?> r = (Map<?, ?>) req;
        Object id = r.get("id");
        Object method = r.get("method");
        if (id == null) {
            // notification — ignore
            return null;
        }
        if (!(method instanceof String)) {
            return error(id, -32600, "invalid request: method missing");
        }
        Object params = r.get("params");
        String m = (String) method;
        try {
            switch (m) {
                case "tools/list":
                    return success(id, Schemas.toolsList());
                case "tools/call":
                    return handleCall(id, params);
                default:
                    return error(id, -32601, "method not found: " + m);
            }
        } catch (Exception e) {
            return error(id, -32603, "internal error: " + e.getMessage());
        }
    }

    private String handleCall(Object id, Object params) {
        if (!(params instanceof Map)) {
            return error(id, -32602, "invalid params");
        }
        Map<?, ?> p = (Map<?, ?>) params;
        Object rawName = p.get("name");
        Object rawArgs = p.get("arguments");
        if (!(rawName instanceof String)) {
            return error(id, -32602, "invalid params: name required");
        }
        Map<String, Object> args;
        if (rawArgs == null) {
            args = new LinkedHashMap<>();
        } else if (rawArgs instanceof Map) {
            @SuppressWarnings("unchecked")
            Map<String, Object> a = (Map<String, Object>) rawArgs;
            args = a;
        } else {
            return error(id, -32602, "invalid params: arguments must be object");
        }
        String tool = ((String) rawName).replace('_', '-');
        switch (tool) {
            case "register-pool":   return doRegisterPool(id, args);
            case "acquire":         return doAcquire(id, args);
            case "release":         return doRelease(id, args);
            case "close-pool":      return doClosePool(id, args);
            case "get-open-count":  return doGetOpenCount(id, args);
            case "get-close-count": return doGetCloseCount(id, args);
            case "get-events":      return doGetEvents(id, args);
            default:                return toolError(id, "not implemented");
        }
    }

    private String doRegisterPool(Object id, Map<String, Object> a) {
        Object key = a.get("key");
        if (key == null) return toolError(id, "invalid argument: key is required");
        Map<String, Object> settingsMap = readSettings(a);
        Integer mc = asInt(settingsMap.get("mc"));
        Integer ct = asInt(settingsMap.get("ct"));
        Integer ka = asInt(settingsMap.get("ka"));
        if (mc == null || ct == null || ka == null) {
            return toolError(id, "invalid argument: mc, ct, ka required");
        }
        long openDelayMs = asLongDefault(a.get("open_delay_ms"), 0L);
        int failCount = asIntDefault(a.get("open_fail_count"), 0);
        boolean hasOnEvent = Boolean.TRUE.equals(a.get("on_event"));

        String poolId = Json.canonical(key);
        PoolInfo info = pools.computeIfAbsent(poolId, k -> {
            PoolInfo pi = new PoolInfo(key, hasOnEvent, openDelayMs, failCount);
            Supplier<String> open = () -> pi.doOpen(poolId);
            Consumer<String> close = pi::doClose;
            Pool.EventListener listener = hasOnEvent ? pi::recordEvent : null;
            Pool<String> pool = registry.register(key, open, close,
                    new PoolSettings(mc, ct, ka), listener);
            pi.pool = pool;
            return pi;
        });

        // Subsequent registrations must not replace original settings/callbacks;
        // we ignore the new parameters and return the existing pool's handle.

        Map<String, Object> result = new LinkedHashMap<>();
        result.put("pool", poolId);
        result.put("pool_id", poolId);
        result.put("content", List.of(textContent(poolId)));
        return success(id, result);
    }

    private String doAcquire(Object id, Map<String, Object> a) {
        String poolId = poolIdOf(a);
        PoolInfo info = pools.get(poolId);
        if (info == null) return toolError(id, "invalid argument: unknown pool");
        String conn;
        try {
            conn = info.pool.acquire();
        } catch (Exception e) {
            String msg = e.getMessage();
            return toolError(id, msg != null ? msg : e.getClass().getSimpleName());
        }
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("connection", conn);
        result.put("connection_id", conn);
        result.put("open_count", info.openCount.get());
        result.put("content", List.of(textContent(conn)));
        return success(id, result);
    }

    private String doRelease(Object id, Map<String, Object> a) {
        String poolId = poolIdOf(a);
        PoolInfo info = pools.get(poolId);
        if (info == null) return toolError(id, "invalid argument: unknown pool");
        Object connObj = a.get("connection");
        if (connObj == null) connObj = a.get("connection_id");
        if (!(connObj instanceof String)) return toolError(id, "invalid argument: connection required");
        info.pool.release((String) connObj);
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("close_count", info.closeCount.get());
        result.put("content", List.of(textContent("")));
        return success(id, result);
    }

    private String doClosePool(Object id, Map<String, Object> a) {
        String poolId = poolIdOf(a);
        PoolInfo info = pools.get(poolId);
        if (info == null) return toolError(id, "invalid argument: unknown pool");
        info.pool.closePool();
        return success(id, new LinkedHashMap<>());
    }

    private String doGetOpenCount(Object id, Map<String, Object> a) {
        String poolId = poolIdOf(a);
        PoolInfo info = pools.get(poolId);
        if (info == null) return toolError(id, "invalid argument: unknown pool");
        int count = info.openCount.get();
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("count", count);
        result.put("content", List.of(textContent(Integer.toString(count))));
        return success(id, result);
    }

    private String doGetCloseCount(Object id, Map<String, Object> a) {
        String poolId = poolIdOf(a);
        PoolInfo info = pools.get(poolId);
        if (info == null) return toolError(id, "invalid argument: unknown pool");
        int count = info.closeCount.get();
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("count", count);
        result.put("content", List.of(textContent(Integer.toString(count))));
        return success(id, result);
    }

    private String doGetEvents(Object id, Map<String, Object> a) {
        String poolId = poolIdOf(a);
        PoolInfo info = pools.get(poolId);
        if (info == null) return toolError(id, "invalid argument: unknown pool");
        List<Map<String, Object>> snapshot;
        synchronized (info.events) {
            snapshot = new ArrayList<>(info.events);
        }
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("events", snapshot);
        result.put("content", List.of(textContent(Json.stringify(snapshot))));
        return success(id, result);
    }

    private static Map<String, Object> textContent(String text) {
        Map<String, Object> c = new LinkedHashMap<>();
        c.put("type", "text");
        c.put("text", text);
        return c;
    }

    private static String poolIdOf(Map<String, Object> a) {
        Object p = a.get("pool");
        if (p == null) p = a.get("pool_id");
        return p instanceof String ? (String) p : (p == null ? null : p.toString());
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> readSettings(Map<String, Object> a) {
        Object s = a.get("settings");
        if (s instanceof Map) {
            return (Map<String, Object>) s;
        }
        Map<String, Object> m = new LinkedHashMap<>();
        if (a.get("mc") != null) m.put("mc", a.get("mc"));
        if (a.get("ct") != null) m.put("ct", a.get("ct"));
        if (a.get("ka") != null) m.put("ka", a.get("ka"));
        return m;
    }

    private static Integer asInt(Object o) {
        if (o == null) return null;
        if (o instanceof Number n) return n.intValue();
        if (o instanceof String s) {
            try { return Integer.parseInt(s); } catch (NumberFormatException e) { return null; }
        }
        return null;
    }

    private static int asIntDefault(Object o, int dflt) {
        Integer v = asInt(o);
        return v == null ? dflt : v;
    }

    private static long asLongDefault(Object o, long dflt) {
        if (o == null) return dflt;
        if (o instanceof Number n) return n.longValue();
        if (o instanceof String s) {
            try { return Long.parseLong(s); } catch (NumberFormatException e) { return dflt; }
        }
        return dflt;
    }

    private static String success(Object id, Object result) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("jsonrpc", "2.0");
        m.put("id", id);
        m.put("result", result);
        return Json.stringify(m);
    }

    private static String error(Object id, int code, String message) {
        Map<String, Object> err = new LinkedHashMap<>();
        err.put("code", code);
        err.put("message", message);
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("jsonrpc", "2.0");
        m.put("id", id);
        m.put("error", err);
        return Json.stringify(m);
    }

    private static String toolError(Object id, String message) {
        return error(id, -32000, message);
    }
}
