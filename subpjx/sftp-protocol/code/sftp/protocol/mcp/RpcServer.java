package sftp.protocol.mcp;

import sftp.protocol.AuthConfig;
import sftp.protocol.Entry;
import sftp.protocol.PoolEvent;
import sftp.protocol.PooledSftpFilesystem;
import sftp.protocol.ReadHandle;
import sftp.protocol.SftpConnector;
import sftp.protocol.SftpException;
import sftp.protocol.SftpFilesystem;
import sftp.protocol.SftpLocation;
import sftp.protocol.SftpPoolRegistry;
import sftp.protocol.SftpSettings;
import sftp.protocol.SftpTransferPool;
import sftp.protocol.WriteHandle;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.time.Duration;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Base64;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicInteger;

final class RpcServer {
    private final ExecutorService clients = Executors.newCachedThreadPool();
    private final AtomicInteger nextId = new AtomicInteger(1);
    private final SftpPoolRegistry registry = new SftpPoolRegistry();
    private final Map<String, SftpFilesystem> filesystems = new HashMap<>();
    private final Map<String, ReadState> readHandles = new HashMap<>();
    private final Map<String, WriteState> writeHandles = new HashMap<>();
    private final Map<String, SftpTransferPool> pools = new HashMap<>();
    private final Map<String, String> poolIdsByEndpoint = new HashMap<>();
    private final Map<SftpTransferPool, String> poolIdsByObject = new IdentityHashMap<>();
    private final Map<String, List<PoolEvent>> poolEvents = new HashMap<>();
    private volatile boolean stopping;
    private ServerSocket server;

    void run() throws IOException {
        server = new ServerSocket(0, 50, InetAddress.getByName("127.0.0.1"));
        System.out.println("MCP_PORT=" + server.getLocalPort());
        System.out.flush();
        while (!stopping) {
            try {
                Socket socket = server.accept();
                clients.submit(() -> handle(socket));
            } catch (IOException e) {
                if (!stopping) {
                    throw e;
                }
            }
        }
        clients.shutdownNow();
    }

    private void handle(Socket socket) {
        try (socket;
             BufferedReader in = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
             BufferedWriter out = new BufferedWriter(new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = in.readLine()) != null && !stopping) {
                Map<String, Object> response = dispatchLine(line);
                if (response != null) {
                    out.write(Json.stringify(response));
                    out.write('\n');
                    out.flush();
                }
            }
        } catch (IOException ignored) {
        }
    }

    @SuppressWarnings("unchecked")
    private Map<String, Object> dispatchLine(String line) {
        Object id = null;
        try {
            Object parsed = Json.parse(line);
            if (!(parsed instanceof Map<?, ?> raw)) {
                return error(null, -32600, "invalid request");
            }
            Map<String, Object> request = (Map<String, Object>) raw;
            id = request.get("id");
            if (!"2.0".equals(request.get("jsonrpc")) || !(request.get("method") instanceof String method)) {
                return error(id, -32600, "invalid request");
            }
            if (id == null) {
                return null;
            }
            return switch (method) {
                case "tools/list" -> result(id, Map.of("tools", tools()));
                case "tools/call" -> call(id, request.get("params"));
                case "aitc/shutdown" -> shutdown(id, request.get("params"));
                default -> error(id, -32601, "method not found: " + method);
            };
        } catch (IllegalArgumentException e) {
            return error(id, -32700, "parse error");
        } catch (Exception e) {
            return error(id, -32603, "internal error");
        }
    }

    @SuppressWarnings("unchecked")
    private Map<String, Object> call(Object id, Object paramsValue) {
        if (!(paramsValue instanceof Map<?, ?> rawParams)) {
            return error(id, -32602, "invalid params");
        }
        Map<String, Object> params = (Map<String, Object>) rawParams;
        if (!(params.get("name") instanceof String name) || !(params.get("arguments") instanceof Map<?, ?> rawArgs)) {
            return error(id, -32602, "invalid params");
        }
        try {
            Map<String, Object> args = (Map<String, Object>) rawArgs;
            return result(id, switch (name) {
                case "close-filesystem" -> closeFilesystem(args);
                case "close-pool-registry" -> closePoolRegistry();
                case "close-read" -> closeRead(args);
                case "close-write" -> closeWrite(args);
                case "create-dir" -> createDir(args);
                case "delete-dir" -> deleteDir(args);
                case "delete-file" -> deleteFile(args);
                case "list-dir" -> listDir(args);
                case "open-read" -> openRead(args);
                case "open-unpooled" -> openUnpooled(args);
                case "open-write" -> openWrite(args);
                case "pool-acquire" -> poolAcquire(args);
                case "pool-events" -> poolEvents(args);
                case "pool-for" -> poolFor(args);
                case "read" -> read(args);
                case "rename" -> rename(args);
                case "set-mod-time" -> setModTime(args);
                case "stat" -> stat(args);
                case "write" -> write(args);
                default -> throw new ToolException("not implemented");
            });
        } catch (ToolException e) {
            return error(id, -32000, e.getMessage());
        } catch (SftpException e) {
            return error(id, -32000, e.category() + ": " + e.getMessage());
        } catch (RuntimeException e) {
            return error(id, -32000, "invalid argument: " + e.getMessage());
        }
    }

    private Map<String, Object> shutdown(Object id, Object params) {
        if (params != null && !(params instanceof Map<?, ?> map && map.isEmpty())) {
            return error(id, -32602, "invalid params");
        }
        stopping = true;
        closeEverything();
        try {
            server.close();
        } catch (IOException ignored) {
        }
        return result(id, Map.of());
    }

    private Map<String, Object> openUnpooled(Map<String, Object> args) throws SftpException {
        String id = id("fs");
        filesystems.put(id, SftpConnector.open_unpooled(location(args), settings(args), auth(args)));
        return Map.of("filesystem_id", id);
    }

    private Map<String, Object> closeFilesystem(Map<String, Object> args) throws ToolException {
        SftpFilesystem fs = filesystems.remove(string(args, "filesystem_id"));
        if (fs == null) {
            throw new ToolException("not_found: filesystem not found");
        }
        fs.close();
        return Map.of();
    }

    private Map<String, Object> listDir(Map<String, Object> args) throws SftpException, ToolException {
        List<Map<String, Object>> entries = new ArrayList<>();
        for (Entry entry : filesystem(args).list_dir(string(args, "path"))) {
            entries.add(entry(entry));
        }
        return Map.of("entries", entries);
    }

    private Map<String, Object> stat(Map<String, Object> args) throws SftpException, ToolException {
        return entry(filesystem(args).stat(string(args, "path")));
    }

    private Map<String, Object> openRead(Map<String, Object> args) throws SftpException, ToolException {
        SftpFilesystem fs = filesystem(args);
        String id = id("read");
        readHandles.put(id, new ReadState(fs, fs.open_read(string(args, "path"))));
        return Map.of("read_handle_id", id);
    }

    private Map<String, Object> read(Map<String, Object> args) throws SftpException, ToolException {
        ReadState state = readHandle(args);
        byte[] bytes = state.filesystem.read(state.handle, intValue(args, "max_bytes", 65536));
        if (bytes == null) {
            return Map.of("eof", true);
        }
        return Map.of(
                "data_base64", Base64.getEncoder().encodeToString(bytes),
                "eof", false);
    }

    private Map<String, Object> closeRead(Map<String, Object> args) throws ToolException {
        ReadState state = readHandles.remove(string(args, "read_handle_id"));
        if (state == null) {
            throw new ToolException("not_found: read handle not found");
        }
        state.filesystem.close_read(state.handle);
        return Map.of();
    }

    private Map<String, Object> openWrite(Map<String, Object> args) throws SftpException, ToolException {
        SftpFilesystem fs = filesystem(args);
        String id = id("write");
        writeHandles.put(id, new WriteState(fs, fs.open_write(string(args, "path"))));
        return Map.of("write_handle_id", id);
    }

    private Map<String, Object> write(Map<String, Object> args) throws SftpException, ToolException {
        WriteState state = writeHandle(args);
        state.filesystem.write(state.handle, Base64.getDecoder().decode(string(args, "data_base64")));
        return Map.of();
    }

    private Map<String, Object> closeWrite(Map<String, Object> args) throws SftpException, ToolException {
        WriteState state = writeHandles.remove(string(args, "write_handle_id"));
        if (state == null) {
            throw new ToolException("not_found: write handle not found");
        }
        state.filesystem.close_write(state.handle);
        return Map.of();
    }

    private Map<String, Object> createDir(Map<String, Object> args) throws SftpException, ToolException {
        filesystem(args).create_dir(string(args, "path"));
        return Map.of();
    }

    private Map<String, Object> deleteDir(Map<String, Object> args) throws SftpException, ToolException {
        filesystem(args).delete_dir(string(args, "path"));
        return Map.of();
    }

    private Map<String, Object> deleteFile(Map<String, Object> args) throws SftpException, ToolException {
        filesystem(args).delete_file(string(args, "path"));
        return Map.of();
    }

    private Map<String, Object> rename(Map<String, Object> args) throws SftpException, ToolException {
        filesystem(args).rename(string(args, "src"), string(args, "dst"));
        return Map.of();
    }

    private Map<String, Object> setModTime(Map<String, Object> args) throws SftpException, ToolException {
        filesystem(args).set_mod_time(string(args, "path"), Instant.parse(string(args, "mod_time")));
        return Map.of();
    }

    private Map<String, Object> poolFor(Map<String, Object> args) {
        SftpLocation location = location(args);
        String endpoint = location.endpointKey();
        String id = poolIdsByEndpoint.get(endpoint);
        List<PoolEvent> events = id == null && boolValue(args, "record_events") ? new ArrayList<>() : null;
        SftpTransferPool pool = registry.pool_for(
                location,
                settings(args),
                auth(args),
                events == null ? null : events::add);
        if (id == null) {
            id = id("pool");
            poolIdsByEndpoint.put(endpoint, id);
            pools.put(id, pool);
            poolIdsByObject.put(pool, id);
            if (events != null) {
                poolEvents.put(id, events);
            }
        } else if (!poolEvents.containsKey(id) && events != null) {
            poolEvents.put(id, events);
        }
        return Map.of("pool_id", poolIdsByObject.getOrDefault(pool, id));
    }

    private Map<String, Object> poolAcquire(Map<String, Object> args) throws SftpException, ToolException {
        SftpTransferPool pool = pool(args);
        String id = id("fs");
        PooledSftpFilesystem fs = pool.acquire();
        filesystems.put(id, fs);
        return Map.of("filesystem_id", id);
    }

    private Map<String, Object> poolEvents(Map<String, Object> args) {
        List<Map<String, Object>> events = new ArrayList<>();
        for (PoolEvent event : poolEvents.getOrDefault(string(args, "pool_id"), List.of())) {
            events.add(Map.of(
                    "endpoint", event.endpoint(),
                    "max_connections", event.max_connections(),
                    "open_connections", event.open_connections()));
        }
        return Map.of("events", events);
    }

    private Map<String, Object> closePoolRegistry() {
        registry.close();
        for (SftpFilesystem fs : filesystems.values()) {
            fs.close();
        }
        filesystems.clear();
        pools.clear();
        poolIdsByEndpoint.clear();
        poolIdsByObject.clear();
        return Map.of();
    }

    private void closeEverything() {
        for (ReadState state : readHandles.values()) {
            state.filesystem.close_read(state.handle);
        }
        readHandles.clear();
        for (WriteState state : writeHandles.values()) {
            try {
                state.filesystem.close_write(state.handle);
            } catch (SftpException ignored) {
            }
        }
        writeHandles.clear();
        closePoolRegistry();
    }

    private SftpFilesystem filesystem(Map<String, Object> args) throws ToolException {
        SftpFilesystem fs = filesystems.get(string(args, "filesystem_id"));
        if (fs == null) {
            throw new ToolException("not_found: filesystem not found");
        }
        return fs;
    }

    private ReadState readHandle(Map<String, Object> args) throws ToolException {
        ReadState state = readHandles.get(string(args, "read_handle_id"));
        if (state == null) {
            throw new ToolException("not_found: read handle not found");
        }
        return state;
    }

    private WriteState writeHandle(Map<String, Object> args) throws ToolException {
        WriteState state = writeHandles.get(string(args, "write_handle_id"));
        if (state == null) {
            throw new ToolException("not_found: write handle not found");
        }
        return state;
    }

    private SftpTransferPool pool(Map<String, Object> args) throws ToolException {
        SftpTransferPool pool = pools.get(string(args, "pool_id"));
        if (pool == null) {
            throw new ToolException("not_found: pool not found");
        }
        return pool;
    }

    private SftpLocation location(Map<String, Object> args) {
        Map<String, Object> location = object(args, "location");
        return new SftpLocation(
                string(location, "user"),
                optionalString(location, "password"),
                string(location, "host"),
                intValue(location, "port", 22),
                string(location, "root_path"));
    }

    private SftpSettings settings(Map<String, Object> args) {
        Map<String, Object> settings = objectOrEmpty(args, "settings");
        return new SftpSettings(
                intValue(settings, "max_connections", 1),
                Duration.ofMillis(durationMillis(settings, "connect_timeout", "connect_timeout_millis", 30_000L)),
                Duration.ofMillis(durationMillis(settings, "idle_keep_alive_ttl", "idle_keep_alive_ttl_millis", 30_000L)));
    }

    private AuthConfig auth(Map<String, Object> args) {
        Map<String, Object> auth = objectOrEmpty(args, "auth_config");
        AuthConfig defaults = AuthConfig.defaults();
        Path knownHosts = auth.get("known_hosts_path") instanceof String path ? Path.of(path) : defaults.known_hosts_path();
        Optional<Path> agent = auth.get("ssh_agent_socket") instanceof String path ? Optional.of(Path.of(path)) : defaults.ssh_agent_socket();
        List<Path> keys = new ArrayList<>();
        if (auth.get("private_key_paths") instanceof List<?> paths) {
            for (Object path : paths) {
                if (path instanceof String s) {
                    keys.add(Path.of(s));
                }
            }
        } else {
            keys.addAll(defaults.private_key_paths());
        }
        return new AuthConfig(knownHosts, agent, keys);
    }

    private Map<String, Object> entry(Entry entry) {
        return Map.of(
                "byte_size", entry.byte_size(),
                "is_dir", entry.is_dir(),
                "mod_time", entry.mod_time().toString(),
                "name", entry.name());
    }

    @SuppressWarnings("unchecked")
    private Map<String, Object> object(Map<String, Object> map, String key) {
        Object value = map.get(key);
        if (!(value instanceof Map<?, ?>)) {
            throw new IllegalArgumentException(key + " is required");
        }
        return (Map<String, Object>) value;
    }

    @SuppressWarnings("unchecked")
    private Map<String, Object> objectOrEmpty(Map<String, Object> map, String key) {
        Object value = map.get(key);
        return value instanceof Map<?, ?> raw ? (Map<String, Object>) raw : Map.of();
    }

    private String string(Map<String, Object> map, String key) {
        Object value = map.get(key);
        if (!(value instanceof String s)) {
            throw new IllegalArgumentException(key + " is required");
        }
        return s;
    }

    private Optional<String> optionalString(Map<String, Object> map, String key) {
        Object value = map.get(key);
        return value instanceof String s && !s.isBlank() ? Optional.of(s) : Optional.empty();
    }

    private boolean boolValue(Map<String, Object> map, String key) {
        Object value = map.get(key);
        return value instanceof Boolean b && b;
    }

    private int intValue(Map<String, Object> map, String key, int fallback) {
        Object value = map.get(key);
        return value instanceof Number n ? n.intValue() : fallback;
    }

    private long durationMillis(Map<String, Object> map, String key, String legacyKey, long fallback) {
        Object value = map.get(key);
        if (!(value instanceof Number)) {
            value = map.get(legacyKey);
        }
        return value instanceof Number n ? n.longValue() : fallback;
    }

    private String id(String prefix) {
        return prefix + "-" + nextId.getAndIncrement();
    }

    private Map<String, Object> result(Object id, Map<String, Object> result) {
        Map<String, Object> response = new TreeMap<>();
        response.put("id", id);
        response.put("jsonrpc", "2.0");
        response.put("result", result);
        return response;
    }

    private Map<String, Object> error(Object id, int code, String message) {
        Map<String, Object> response = new TreeMap<>();
        response.put("error", Map.of("code", code, "message", message));
        response.put("id", id);
        response.put("jsonrpc", "2.0");
        return response;
    }

    private List<Map<String, Object>> tools() {
        return List.of(
                tool("close-filesystem"),
                tool("close-pool-registry"),
                tool("close-read"),
                tool("close-write"),
                tool("create-dir"),
                tool("delete-dir"),
                tool("delete-file"),
                tool("list-dir"),
                tool("open-read"),
                tool("open-unpooled"),
                tool("open-write"),
                tool("pool-acquire"),
                tool("pool-events"),
                tool("pool-for"),
                tool("read"),
                tool("rename"),
                tool("set-mod-time"),
                tool("stat"),
                tool("write"));
    }

    private Map<String, Object> tool(String name) {
        return new TreeMap<>(Map.of(
                "description", name,
                "inputSchema", objectSchema(),
                "name", name,
                "outputSchema", objectSchema()));
    }

    private Map<String, Object> objectSchema() {
        return new TreeMap<>(Map.of(
                "additionalProperties", false,
                "properties", Map.of(),
                "type", "object"));
    }

    private record ReadState(SftpFilesystem filesystem, ReadHandle handle) {
    }

    private record WriteState(SftpFilesystem filesystem, WriteHandle handle) {
    }

    private static final class ToolException extends Exception {
        ToolException(String message) {
            super(message);
        }
    }
}
