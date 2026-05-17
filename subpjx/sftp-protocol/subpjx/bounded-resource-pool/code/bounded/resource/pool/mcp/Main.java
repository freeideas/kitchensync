package bounded.resource.pool.mcp;

import bounded.resource.pool.BoundedPool;
import bounded.resource.pool.BoundedPoolRegistry;
import bounded.resource.pool.PoolEvent;
import bounded.resource.pool.PoolSettings;
import bounded.resource.pool.ResourceFactory;
import bounded.resource.pool.ResourceLease;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.TreeMap;

public final class Main {
    private ServerSocket server;
    private final AtomicInteger nextId = new AtomicInteger();

    // Low-level surface (kebab-case): explicit registry/factory/pool IDs.
    private final Map<String, BoundedPoolRegistry<String, String>> registries = new ConcurrentHashMap<>();
    private final Map<String, FakeFactory> factories = new ConcurrentHashMap<>();
    private final Map<String, BoundedPool<String>> pools = new ConcurrentHashMap<>();
    private final Map<String, List<Map<String, Object>>> events = new ConcurrentHashMap<>();
    private final Map<String, String> poolIdsByRegistryKey = new ConcurrentHashMap<>();

    // High-level surface (snake_case): single default registry, key-only addressing.
    private final Object defaultRegistryLock = new Object();
    private volatile BoundedPoolRegistry<String, String> defaultRegistry;
    private final Map<String, BoundedPool<String>> defaultPools = new ConcurrentHashMap<>();
    private final Map<String, FakeFactory> defaultFactories = new ConcurrentHashMap<>();
    private final Map<String, List<Map<String, Object>>> defaultEvents = new ConcurrentHashMap<>();

    private final Map<String, ResourceLease<String>> leases = new ConcurrentHashMap<>();

    public static void main(String[] args) throws Exception {
        new Main().run();
    }

    private void run() throws Exception {
        server = new ServerSocket(0, 50, java.net.InetAddress.getByName("127.0.0.1"));
        System.out.println("MCP_PORT=" + server.getLocalPort());
        System.out.flush();
        while (!server.isClosed()) {
            try {
                Socket socket = server.accept();
                Thread thread = new Thread(() -> serve(socket), "bounded-resource-pool-mcp-client");
                thread.start();
            } catch (Exception ignored) {
                if (!server.isClosed()) {
                    throw ignored;
                }
            }
        }
    }

    private void serve(Socket socket) {
        Set<String> connectionLeases = new java.util.HashSet<>();
        try (socket;
                BufferedReader reader = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
                BufferedWriter writer = new BufferedWriter(new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                Response response = handle(line);
                if (response.createdLeaseId() != null) {
                    connectionLeases.add(response.createdLeaseId());
                }
                if (response.closedLeaseId() != null) {
                    connectionLeases.remove(response.closedLeaseId());
                }
                if (response.body() != null) {
                    try {
                        writer.write(Json.write(response.body()));
                        writer.write('\n');
                        writer.flush();
                    } catch (IOException writeFailure) {
                        connectionLeases.remove(response.createdLeaseId());
                        releaseOrphanedLease(response.createdLeaseId());
                        throw writeFailure;
                    }
                }
                if (response.shutdown()) {
                    server.close();
                    System.exit(0);
                }
            }
        } catch (Exception ignored) {
        } finally {
            for (String leaseId : connectionLeases) {
                releaseOrphanedLease(leaseId);
            }
        }
    }

    private void releaseOrphanedLease(String leaseId) {
        if (leaseId == null) {
            return;
        }
        ResourceLease<String> lease = leases.remove(leaseId);
        if (lease == null) {
            return;
        }
        try {
            lease.close();
        } catch (Exception ignored) {
        }
    }

    private Response handle(String line) {
        Object parsed;
        try {
            parsed = Json.parse(line);
        } catch (RuntimeException failure) {
            return new Response(error(null, -32700, "parse error"), false, null, null);
        }
        if (!(parsed instanceof Map<?, ?> request)) {
            return new Response(error(null, -32600, "invalid request"), false, null, null);
        }
        Object id = request.get("id");
        if (id == null) {
            return new Response(null, false, null, null);
        }
        if (!"2.0".equals(request.get("jsonrpc")) || !(request.get("method") instanceof String method)) {
            return new Response(error(id, -32600, "invalid request"), false, null, null);
        }
        try {
            return switch (method) {
                case "tools/list" -> new Response(result(id, toolsList()), false, null, null);
                case "tools/call" -> toolsCall(id, request.get("params"));
                case "aitc/shutdown" -> shutdown(id, request.get("params"), request.containsKey("params"));
                default -> new Response(error(id, -32601, "method not found: " + method), false, null, null);
            };
        } catch (RuntimeException failure) {
            return new Response(error(id, -32603, "internal error"), false, null, null);
        }
    }

    private Response toolsCall(Object id, Object params) {
        if (!(params instanceof Map<?, ?> map)
                || !(map.get("name") instanceof String name)
                || !(map.get("arguments") instanceof Map<?, ?> arguments)) {
            return new Response(error(id, -32602, "invalid params"), false, null, null);
        }
        try {
            Object payload = callTool(name, arguments);
            String createdLeaseId = null;
            if (payload instanceof Map<?, ?> resultMap && resultMap.get("lease_id") instanceof String s) {
                createdLeaseId = s;
            }
            String closedLeaseId = null;
            if ((name.equals("lease_close") || name.equals("lease-close")) && arguments.get("lease_id") instanceof String s) {
                closedLeaseId = s;
            }
            return new Response(wrappedResult(id, payload), false, createdLeaseId, closedLeaseId);
        } catch (IllegalArgumentException failure) {
            return new Response(error(id, -32602, "argument validation error: " + failure.getMessage()), false, null, null);
        } catch (Exception failure) {
            return new Response(error(id, -32000, failure.getMessage()), false, null, null);
        }
    }

    private Object callTool(String name, Map<?, ?> arguments) throws Exception {
        return switch (name) {
            case "registry-new" -> registryNew();
            case "fake-factory-new" -> factoryNew(arguments);
            case "pool-for" -> poolFor(arguments);
            case "pool-acquire" -> poolAcquire(arguments);
            case "lease-close", "lease_close" -> leaseClose(arguments);
            case "lease-invalidate", "lease_invalidate" -> leaseInvalidate(arguments);
            case "registry-close" -> registryClose(arguments);
            case "factory-stats" -> factoryStats(arguments);
            case "factory-fail-next-opens" -> factoryFailNextOpens(arguments);
            case "factory-fail-next-closes" -> factoryFailNextCloses(arguments);
            case "pool-events" -> poolEvents(arguments);
            case "pool_for" -> highLevelPoolFor(arguments);
            case "acquire" -> highLevelAcquire(arguments);
            case "get_events" -> highLevelGetEvents(arguments);
            case "registry_close" -> highLevelRegistryClose();
            default -> throw new UnsupportedOperationException("not implemented");
        };
    }

    private Map<String, Object> registryNew() {
        String id = newId("registry");
        registries.put(id, new BoundedPoolRegistry<>());
        return Map.of("registry_id", id);
    }

    private Map<String, Object> factoryNew(Map<?, ?> arguments) {
        String prefix = stringArgument(arguments, "prefix", "resource");
        String id = newId("factory");
        factories.put(id, new FakeFactory(prefix));
        return Map.of("factory_id", id);
    }

    private Map<String, Object> poolFor(Map<?, ?> arguments) {
        String registryId = requiredString(arguments, "registry_id");
        String key = requiredString(arguments, "key");
        String factoryId = requiredString(arguments, "factory_id");
        BoundedPoolRegistry<String, String> registry = require(registries, registryId, "registry");
        FakeFactory factory = require(factories, factoryId, "factory");
        PoolSettings settings = settings(arguments);
        boolean listenerEnabled = booleanArgument(arguments, "listener", false);
        String lookupKey = registryId + "\n" + key;
        String existing = poolIdsByRegistryKey.get(lookupKey);
        if (existing != null) {
            return Map.of("pool_id", existing);
        }
        String poolId = newId("pool");
        List<Map<String, Object>> poolEvents = new CopyOnWriteArrayList<>();
        BoundedPool<String> pool = registry.pool_for(
                key,
                settings,
                factory,
                listenerEnabled ? event -> poolEvents.add(eventMap(event)) : null);
        pools.put(poolId, pool);
        events.put(poolId, poolEvents);
        poolIdsByRegistryKey.put(lookupKey, poolId);
        return Map.of("pool_id", poolId);
    }

    private Map<String, Object> poolAcquire(Map<?, ?> arguments) throws Exception {
        String poolId = requiredString(arguments, "pool_id");
        BoundedPool<String> pool = require(pools, poolId, "pool");
        ResourceLease<String> lease = pool.acquire();
        String leaseId = newId("lease");
        leases.put(leaseId, lease);
        return Map.of("lease_id", leaseId, "resource", lease.resource());
    }

    private Map<String, Object> leaseClose(Map<?, ?> arguments) {
        String leaseId = requiredString(arguments, "lease_id");
        ResourceLease<String> lease = leases.remove(leaseId);
        if (lease == null) {
            throw new IllegalArgumentException("unknown lease");
        }
        lease.close();
        return Map.of();
    }

    private Map<String, Object> leaseInvalidate(Map<?, ?> arguments) {
        String leaseId = requiredString(arguments, "lease_id");
        require(leases, leaseId, "lease").invalidate();
        return Map.of();
    }

    private Map<String, Object> registryClose(Map<?, ?> arguments) {
        String registryId = requiredString(arguments, "registry_id");
        require(registries, registryId, "registry").close();
        return Map.of();
    }

    private Map<String, Object> factoryStats(Map<?, ?> arguments) {
        String factoryId = requiredString(arguments, "factory_id");
        FakeFactory factory = require(factories, factoryId, "factory");
        return Map.of("open_count", factory.openCount(), "close_count", factory.closeCount());
    }

    private Map<String, Object> factoryFailNextOpens(Map<?, ?> arguments) {
        String factoryId = requiredString(arguments, "factory_id");
        require(factories, factoryId, "factory").failNextOpens(intArgument(arguments, "count"));
        return Map.of();
    }

    private Map<String, Object> factoryFailNextCloses(Map<?, ?> arguments) {
        String factoryId = requiredString(arguments, "factory_id");
        require(factories, factoryId, "factory").failNextCloses(intArgument(arguments, "count"));
        return Map.of();
    }

    private Map<String, Object> poolEvents(Map<?, ?> arguments) {
        String poolId = requiredString(arguments, "pool_id");
        return Map.of("events", new ArrayList<>(require(events, poolId, "pool events")));
    }

    private BoundedPoolRegistry<String, String> ensureDefaultRegistry() {
        BoundedPoolRegistry<String, String> snapshot = defaultRegistry;
        if (snapshot != null) {
            return snapshot;
        }
        synchronized (defaultRegistryLock) {
            if (defaultRegistry == null) {
                defaultRegistry = new BoundedPoolRegistry<>();
            }
            return defaultRegistry;
        }
    }

    private Map<String, Object> highLevelPoolFor(Map<?, ?> arguments) {
        String key = requiredString(arguments, "key");
        BoundedPool<String> existing = defaultPools.get(key);
        if (existing != null) {
            return Map.of();
        }

        int maxResources = intArgument(arguments, "max_resources");
        Duration ttl = Duration.parse(requiredString(arguments, "idle_keep_alive_ttl"));
        PoolSettings settings = new PoolSettings(maxResources, ttl);
        BoundedPoolRegistry<String, String> registry = ensureDefaultRegistry();
        synchronized (defaultPools) {
            existing = defaultPools.get(key);
            if (existing == null) {
                FakeFactory factory = new FakeFactory("resource-" + key);
                if (arguments.containsKey("fail_open_count")) {
                    factory.failNextOpens(intArgument(arguments, "fail_open_count"));
                }
                List<Map<String, Object>> evList = new CopyOnWriteArrayList<>();
                BoundedPool<String> pool = registry.pool_for(
                        key,
                        settings,
                        factory,
                        event -> evList.add(eventMap(event)));
                defaultFactories.put(key, factory);
                defaultEvents.put(key, evList);
                defaultPools.put(key, pool);
            }
        }
        return Map.of();
    }

    private Map<String, Object> highLevelAcquire(Map<?, ?> arguments) throws Exception {
        String key = requiredString(arguments, "key");
        BoundedPool<String> pool = defaultPools.get(key);
        if (pool == null) {
            throw new IllegalArgumentException("unknown key: " + key);
        }
        ResourceLease<String> lease = pool.acquire();
        String leaseId = newId("lease");
        leases.put(leaseId, lease);
        Map<String, Object> result = new TreeMap<>();
        result.put("lease_id", leaseId);
        result.put("resource", lease.resource());
        return result;
    }

    private List<Map<String, Object>> highLevelGetEvents(Map<?, ?> arguments) {
        String key = requiredString(arguments, "key");
        List<Map<String, Object>> evs = defaultEvents.get(key);
        return evs == null ? List.of() : new ArrayList<>(evs);
    }

    private Map<String, Object> highLevelRegistryClose() {
        BoundedPoolRegistry<String, String> snapshot;
        synchronized (defaultRegistryLock) {
            snapshot = defaultRegistry;
        }
        if (snapshot != null) {
            snapshot.close();
        }
        return Map.of();
    }

    private PoolSettings settings(Map<?, ?> arguments) {
        int maxResources;
        Duration ttl;
        if (arguments.get("settings") instanceof Map<?, ?> settings) {
            maxResources = intArgument(settings, "max_resources");
            ttl = Duration.parse(requiredString(settings, "idle_keep_alive_ttl"));
        } else {
            maxResources = intArgument(arguments, "max_resources");
            ttl = Duration.parse(requiredString(arguments, "idle_keep_alive_ttl"));
        }
        return new PoolSettings(maxResources, ttl);
    }

    private Map<String, Object> eventMap(PoolEvent<String> event) {
        return Map.of(
                "key", event.key(),
                "open_resources", event.open_resources(),
                "max_resources", event.max_resources());
    }

    private String newId(String prefix) {
        return prefix + "-" + nextId.incrementAndGet();
    }

    private String requiredString(Map<?, ?> map, String name) {
        Object value = map.get(name);
        if (value == null) {
            throw new IllegalArgumentException("missing " + name);
        }
        return Objects.toString(value);
    }

    private String stringArgument(Map<?, ?> map, String name, String fallback) {
        Object value = map.get(name);
        return value == null ? fallback : Objects.toString(value);
    }

    private int intArgument(Map<?, ?> map, String name) {
        Object value = map.get(name);
        if (value instanceof Number number) {
            return number.intValue();
        }
        if (value instanceof String string) {
            return Integer.parseInt(string);
        }
        throw new IllegalArgumentException("missing " + name);
    }

    private boolean booleanArgument(Map<?, ?> map, String name, boolean fallback) {
        Object value = map.get(name);
        return value instanceof Boolean bool ? bool : fallback;
    }

    private <T> T require(Map<String, T> map, String id, String label) {
        T value = map.get(id);
        if (value == null) {
            throw new IllegalArgumentException("unknown " + label);
        }
        return value;
    }

    private Response shutdown(Object id, Object params, boolean hasParams) {
        if (hasParams && !(params instanceof Map<?, ?> map && map.isEmpty())) {
            return new Response(error(id, -32602, "invalid params"), false, null, null);
        }
        return new Response(result(id, Map.of()), true, null, null);
    }

    private Map<String, Object> toolsList() {
        return Map.of("tools", List.of(
                tool("factory-fail-next-closes"),
                tool("factory-fail-next-opens"),
                tool("factory-stats"),
                tool("fake-factory-new"),
                tool("lease-close"),
                tool("lease-invalidate"),
                tool("pool-acquire"),
                tool("pool-events"),
                tool("pool-for"),
                tool("registry-close"),
                tool("registry-new")));
    }

    private Map<String, Object> tool(String name) {
        return Map.of("name", name);
    }

    private Map<String, Object> result(Object id, Object result) {
        Map<String, Object> response = new TreeMap<>();
        response.put("id", id);
        response.put("jsonrpc", "2.0");
        response.put("result", result);
        return response;
    }

    private Map<String, Object> wrappedResult(Object id, Object payload) {
        Map<String, Object> content = new TreeMap<>();
        content.put("type", "text");
        content.put("text", Json.write(payload));
        Map<String, Object> wrapped = new TreeMap<>();
        wrapped.put("content", List.of(content));
        return result(id, wrapped);
    }

    private Map<String, Object> error(Object id, int code, String message) {
        Map<String, Object> error = new TreeMap<>();
        error.put("code", code);
        error.put("message", message);
        Map<String, Object> response = new TreeMap<>();
        response.put("error", error);
        response.put("id", id);
        response.put("jsonrpc", "2.0");
        return response;
    }

    private record Response(Map<String, Object> body, boolean shutdown, String createdLeaseId, String closedLeaseId) {
    }

    private static final class FakeFactory implements ResourceFactory<String, String> {
        private final String prefix;
        private int openCount;
        private int closeCount;
        private int openFailures;
        private int closeFailures;

        private FakeFactory(String prefix) {
            this.prefix = prefix;
        }

        @Override
        public synchronized String open(String key) throws Exception {
            if (openFailures > 0) {
                openFailures--;
                throw new Exception("open failed");
            }
            openCount++;
            return prefix + "-" + openCount;
        }

        @Override
        public synchronized void close(String resource) throws Exception {
            closeCount++;
            if (closeFailures > 0) {
                closeFailures--;
                throw new Exception("close failed");
            }
        }

        private synchronized int openCount() {
            return openCount;
        }

        private synchronized int closeCount() {
            return closeCount;
        }

        private synchronized void failNextOpens(int count) {
            openFailures = count;
        }

        private synchronized void failNextCloses(int count) {
            closeFailures = count;
        }
    }
}
