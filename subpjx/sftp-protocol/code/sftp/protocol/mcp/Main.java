package sftp.protocol.mcp;

import sftp.protocol.*;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;

public final class Main {

    // tools sorted alphabetically; JSON object keys sorted lexicographically
    private static final String TOOLS_LIST_RESULT =
        "{\"tools\":["
        + "{\"description\":\"Acquire a connection handle from the pool for the given SFTP URL.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"url\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"url\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"acquire\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"handleId\"],"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Close a read handle.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"readHandleId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"readHandleId\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"close-read\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Flush and close a write handle.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"writeHandleId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"writeHandleId\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"close-write\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Set pool configuration before first use.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"connectTimeoutSeconds\":{\"type\":\"number\"},"
            + "\"idleKeepaliveSeconds\":{\"type\":\"number\"},"
            + "\"maxConnections\":{\"type\":\"integer\"}"
          + "},"
          + "\"required\":[],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"configure\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Create a directory and any missing parent directories (idempotent).\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"},"
            + "\"path\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"handleId\",\"path\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"create-dir\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Remove an empty directory.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"},"
            + "\"path\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"handleId\",\"path\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"delete-dir\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Remove a regular file.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"},"
            + "\"path\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"handleId\",\"path\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"delete-file\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"List the immediate children of a directory.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"},"
            + "\"path\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"handleId\",\"path\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"list-dir\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"entries\":{"
              + "\"items\":{"
                + "\"additionalProperties\":false,"
                + "\"properties\":{"
                  + "\"byteSize\":{\"type\":\"integer\"},"
                  + "\"isDir\":{\"type\":\"boolean\"},"
                  + "\"modTimeEpochSeconds\":{\"type\":\"integer\"},"
                  + "\"name\":{\"type\":\"string\"}"
                + "},"
                + "\"required\":[\"byteSize\",\"isDir\",\"modTimeEpochSeconds\",\"name\"],"
                + "\"type\":\"object\""
              + "},"
              + "\"type\":\"array\""
            + "}"
          + "},"
          + "\"required\":[\"entries\"],"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Open a regular file for streaming reads and return a read handle.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"},"
            + "\"path\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"handleId\",\"path\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"open-read\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"readHandleId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"readHandleId\"],"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Open a regular file for streaming writes and return a write handle.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"},"
            + "\"path\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"handleId\",\"path\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"open-write\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"writeHandleId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"writeHandleId\"],"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Pull the next chunk from a read handle; eof is true when no more data.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"maxBytes\":{\"minimum\":1,\"type\":\"integer\"},"
            + "\"readHandleId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"maxBytes\",\"readHandleId\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"read\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"data\":{\"type\":\"string\"},"
            + "\"eof\":{\"type\":\"boolean\"}"
          + "},"
          + "\"required\":[\"data\",\"eof\"],"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Release a connection handle back to the pool.\","
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
        + "{\"description\":\"Rename a file or directory on the remote filesystem.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"dst\":{\"type\":\"string\"},"
            + "\"handleId\":{\"type\":\"string\"},"
            + "\"src\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"dst\",\"handleId\",\"src\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"rename\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Set the modification time of a file or directory.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"},"
            + "\"modTimeEpochSeconds\":{\"type\":\"integer\"},"
            + "\"path\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"handleId\",\"modTimeEpochSeconds\",\"path\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"set-mod-time\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Close all sessions and tear down the pool.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"shutdown\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Return mod_time, byte_size, and is_dir for a path, or error if not found.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"handleId\":{\"type\":\"string\"},"
            + "\"path\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"handleId\",\"path\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"stat\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"byteSize\":{\"type\":\"integer\"},"
            + "\"isDir\":{\"type\":\"boolean\"},"
            + "\"modTimeEpochSeconds\":{\"type\":\"integer\"}"
          + "},"
          + "\"required\":[\"byteSize\",\"isDir\",\"modTimeEpochSeconds\"],"
          + "\"type\":\"object\""
        + "}},"
        + "{\"description\":\"Push bytes to a write handle; data is base64-encoded.\","
        + "\"inputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{"
            + "\"data\":{\"type\":\"string\"},"
            + "\"writeHandleId\":{\"type\":\"string\"}"
          + "},"
          + "\"required\":[\"data\",\"writeHandleId\"],"
          + "\"type\":\"object\""
        + "},"
        + "\"name\":\"write\","
        + "\"outputSchema\":{"
          + "\"additionalProperties\":false,"
          + "\"properties\":{},"
          + "\"type\":\"object\""
        + "}}"
        + "]}";

    private static final AtomicInteger counter = new AtomicInteger(0);

    // Pool state - guarded by poolLock for lazy init
    private static final Object poolLock = new Object();
    private static volatile SftpPool pool;
    private static int cfgMaxConn = 10;
    private static double cfgConnTimeout = 30.0;
    private static double cfgIdleKeepalive = 30.0;

    private static final ConcurrentHashMap<String, ConnectionHandle> connections = new ConcurrentHashMap<>();
    private static final ConcurrentHashMap<String, ReadHandle> readHandles = new ConcurrentHashMap<>();
    private static final ConcurrentHashMap<String, WriteHandle> writeHandles = new ConcurrentHashMap<>();

    private static SftpPool getPool() {
        if (pool == null) {
            synchronized (poolLock) {
                if (pool == null) {
                    pool = new SftpPool(new SftpPoolConfig(cfgMaxConn, cfgConnTimeout, cfgIdleKeepalive));
                }
            }
        }
        return pool;
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
            if (id == null) return null;

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
        Map<String, Object> args = (Map<String, Object>) argsObj;

        try {
            return switch (name) {
                case "acquire" -> toolAcquire(id, args);
                case "close-read" -> toolCloseRead(id, args);
                case "close-write" -> toolCloseWrite(id, args);
                case "configure" -> toolConfigure(id, args);
                case "create-dir" -> toolCreateDir(id, args);
                case "delete-dir" -> toolDeleteDir(id, args);
                case "delete-file" -> toolDeleteFile(id, args);
                case "list-dir" -> toolListDir(id, args);
                case "open-read" -> toolOpenRead(id, args);
                case "open-write" -> toolOpenWrite(id, args);
                case "read" -> toolRead(id, args);
                case "release" -> toolRelease(id, args);
                case "rename" -> toolRename(id, args);
                case "set-mod-time" -> toolSetModTime(id, args);
                case "shutdown" -> toolShutdown(id, args);
                case "stat" -> toolStat(id, args);
                case "write" -> toolWrite(id, args);
                default -> errorResponse(id, -32000, "not implemented");
            };
        } catch (SftpNotFoundException e) {
            return errorResponse(id, -32000, "not found: " + e.getMessage());
        } catch (SftpPermissionDeniedException e) {
            return errorResponse(id, -32000, "permission denied: " + e.getMessage());
        } catch (SftpException e) {
            return errorResponse(id, -32000, "io error: " + e.getMessage());
        } catch (Exception e) {
            return errorResponse(id, -32603, "internal error: " + e.getMessage());
        }
    }

    private static String toolConfigure(Object id, Map<String, Object> args) {
        synchronized (poolLock) {
            if (pool != null) return errorResponse(id, -32000, "pool already started");
            Object mc = args.get("maxConnections");
            Object ct = args.get("connectTimeoutSeconds");
            Object ik = args.get("idleKeepaliveSeconds");
            if (mc instanceof Number n) cfgMaxConn = n.intValue();
            if (ct instanceof Number n) cfgConnTimeout = n.doubleValue();
            if (ik instanceof Number n) cfgIdleKeepalive = n.doubleValue();
        }
        return successResponse(id, "{}");
    }

    private static String toolAcquire(Object id, Map<String, Object> args) {
        String url = requireString(args, "url");
        if (url == null) return errorResponse(id, -32000, "invalid argument: url required");
        try {
            ConnectionHandle handle = getPool().acquire(url);
            String handleId = "c" + counter.incrementAndGet();
            connections.put(handleId, handle);
            return successResponse(id, "{\"handleId\":" + jsonString(handleId) + "}");
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return errorResponse(id, -32000, "interrupted");
        }
    }

    private static String toolRelease(Object id, Map<String, Object> args) {
        String handleId = requireString(args, "handleId");
        if (handleId == null) return errorResponse(id, -32000, "invalid argument: handleId required");
        ConnectionHandle handle = connections.remove(handleId);
        if (handle == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");
        getPool().release(handle);
        return successResponse(id, "{}");
    }

    private static String toolShutdown(Object id, Map<String, Object> args) {
        synchronized (poolLock) {
            if (pool != null) {
                pool.shutdown();
                pool = null;
            }
        }
        connections.clear();
        readHandles.clear();
        writeHandles.clear();
        return successResponse(id, "{}");
    }

    private static String toolListDir(Object id, Map<String, Object> args) {
        ConnectionHandle handle = getConnectionHandle(id, args);
        if (handle == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");
        String path = requireString(args, "path");
        if (path == null) return errorResponse(id, -32000, "invalid argument: path required");

        List<DirEntry> entries = handle.listDir(path);
        StringBuilder sb = new StringBuilder("{\"entries\":[");
        for (int i = 0; i < entries.size(); i++) {
            DirEntry e = entries.get(i);
            if (i > 0) sb.append(',');
            sb.append("{\"byteSize\":").append(e.byteSize())
              .append(",\"isDir\":").append(e.isDir())
              .append(",\"modTimeEpochSeconds\":").append(e.modTime().getEpochSecond())
              .append(",\"name\":").append(jsonString(e.name()))
              .append('}');
        }
        sb.append("]}");
        return successResponse(id, sb.toString());
    }

    private static String toolStat(Object id, Map<String, Object> args) {
        ConnectionHandle handle = getConnectionHandle(id, args);
        if (handle == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");
        String path = requireString(args, "path");
        if (path == null) return errorResponse(id, -32000, "invalid argument: path required");

        StatResult result = handle.stat(path);
        return successResponse(id, "{\"byteSize\":" + result.byteSize()
            + ",\"isDir\":" + result.isDir()
            + ",\"modTimeEpochSeconds\":" + result.modTime().getEpochSecond() + "}");
    }

    private static String toolOpenRead(Object id, Map<String, Object> args) {
        ConnectionHandle handle = getConnectionHandle(id, args);
        if (handle == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");
        String path = requireString(args, "path");
        if (path == null) return errorResponse(id, -32000, "invalid argument: path required");

        ReadHandle rh = handle.openRead(path);
        String rhId = "r" + counter.incrementAndGet();
        readHandles.put(rhId, rh);
        return successResponse(id, "{\"readHandleId\":" + jsonString(rhId) + "}");
    }

    private static String toolRead(Object id, Map<String, Object> args) {
        String rhId = requireString(args, "readHandleId");
        if (rhId == null) return errorResponse(id, -32000, "invalid argument: readHandleId required");
        Object maxBytesObj = args.get("maxBytes");
        if (!(maxBytesObj instanceof Number)) return errorResponse(id, -32000, "invalid argument: maxBytes required");
        int maxBytes = ((Number) maxBytesObj).intValue();

        ReadHandle rh = readHandles.get(rhId);
        if (rh == null) return errorResponse(id, -32000, "invalid argument: unknown readHandleId");

        try {
            byte[] data = rh.read(maxBytes);
            if (data == null) {
                return successResponse(id, "{\"data\":\"\",\"eof\":true}");
            }
            String encoded = Base64.getEncoder().encodeToString(data);
            return successResponse(id, "{\"data\":" + jsonString(encoded) + ",\"eof\":false}");
        } catch (SftpException e) {
            return errorResponse(id, -32000, "io error: " + e.getMessage());
        } catch (Exception e) {
            return errorResponse(id, -32000, "io error: " + e.getMessage());
        }
    }

    private static String toolCloseRead(Object id, Map<String, Object> args) {
        String rhId = requireString(args, "readHandleId");
        if (rhId == null) return errorResponse(id, -32000, "invalid argument: readHandleId required");
        ReadHandle rh = readHandles.remove(rhId);
        if (rh == null) return errorResponse(id, -32000, "invalid argument: unknown readHandleId");
        try { rh.close(); } catch (Exception ignored) {}
        return successResponse(id, "{}");
    }

    private static String toolOpenWrite(Object id, Map<String, Object> args) {
        ConnectionHandle handle = getConnectionHandle(id, args);
        if (handle == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");
        String path = requireString(args, "path");
        if (path == null) return errorResponse(id, -32000, "invalid argument: path required");

        WriteHandle wh = handle.openWrite(path);
        String whId = "w" + counter.incrementAndGet();
        writeHandles.put(whId, wh);
        return successResponse(id, "{\"writeHandleId\":" + jsonString(whId) + "}");
    }

    private static String toolWrite(Object id, Map<String, Object> args) {
        String whId = requireString(args, "writeHandleId");
        if (whId == null) return errorResponse(id, -32000, "invalid argument: writeHandleId required");
        String dataB64 = requireString(args, "data");
        if (dataB64 == null) return errorResponse(id, -32000, "invalid argument: data required");

        WriteHandle wh = writeHandles.get(whId);
        if (wh == null) return errorResponse(id, -32000, "invalid argument: unknown writeHandleId");

        try {
            byte[] bytes = Base64.getDecoder().decode(dataB64);
            wh.write(bytes);
            return successResponse(id, "{}");
        } catch (SftpException e) {
            return errorResponse(id, -32000, "io error: " + e.getMessage());
        } catch (Exception e) {
            return errorResponse(id, -32000, "io error: " + e.getMessage());
        }
    }

    private static String toolCloseWrite(Object id, Map<String, Object> args) {
        String whId = requireString(args, "writeHandleId");
        if (whId == null) return errorResponse(id, -32000, "invalid argument: writeHandleId required");
        WriteHandle wh = writeHandles.remove(whId);
        if (wh == null) return errorResponse(id, -32000, "invalid argument: unknown writeHandleId");
        try { wh.close(); } catch (Exception ignored) {}
        return successResponse(id, "{}");
    }

    private static String toolRename(Object id, Map<String, Object> args) {
        ConnectionHandle handle = getConnectionHandle(id, args);
        if (handle == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");
        String src = requireString(args, "src");
        String dst = requireString(args, "dst");
        if (src == null) return errorResponse(id, -32000, "invalid argument: src required");
        if (dst == null) return errorResponse(id, -32000, "invalid argument: dst required");
        handle.rename(src, dst);
        return successResponse(id, "{}");
    }

    private static String toolDeleteFile(Object id, Map<String, Object> args) {
        ConnectionHandle handle = getConnectionHandle(id, args);
        if (handle == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");
        String path = requireString(args, "path");
        if (path == null) return errorResponse(id, -32000, "invalid argument: path required");
        handle.deleteFile(path);
        return successResponse(id, "{}");
    }

    private static String toolCreateDir(Object id, Map<String, Object> args) {
        ConnectionHandle handle = getConnectionHandle(id, args);
        if (handle == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");
        String path = requireString(args, "path");
        if (path == null) return errorResponse(id, -32000, "invalid argument: path required");
        handle.createDir(path);
        return successResponse(id, "{}");
    }

    private static String toolDeleteDir(Object id, Map<String, Object> args) {
        ConnectionHandle handle = getConnectionHandle(id, args);
        if (handle == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");
        String path = requireString(args, "path");
        if (path == null) return errorResponse(id, -32000, "invalid argument: path required");
        handle.deleteDir(path);
        return successResponse(id, "{}");
    }

    private static String toolSetModTime(Object id, Map<String, Object> args) {
        ConnectionHandle handle = getConnectionHandle(id, args);
        if (handle == null) return errorResponse(id, -32000, "invalid argument: unknown handleId");
        String path = requireString(args, "path");
        if (path == null) return errorResponse(id, -32000, "invalid argument: path required");
        Object epochObj = args.get("modTimeEpochSeconds");
        if (!(epochObj instanceof Number)) return errorResponse(id, -32000, "invalid argument: modTimeEpochSeconds required");
        long epochSeconds = ((Number) epochObj).longValue();
        handle.setModTime(path, Instant.ofEpochSecond(epochSeconds));
        return successResponse(id, "{}");
    }

    private static ConnectionHandle getConnectionHandle(Object id, Map<String, Object> args) {
        String handleId = requireString(args, "handleId");
        if (handleId == null) return null;
        return connections.get(handleId);
    }

    private static String requireString(Map<String, Object> args, String key) {
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

    static String jsonString(String s) {
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
            pos++;
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
            pos++;
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
            pos++;
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
