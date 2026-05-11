package sftp.protocol.mcp;

import sftp.protocol.Transport;
import ssh.sftp.session.SftpFailureException;
import ssh.sftp.session.SshSftp;

import java.io.IOException;
import java.util.ArrayList;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

public final class Tools {
    private Tools() {}

    public static Map<String, Object> list() {
        List<Map<String, Object>> tools = new ArrayList<>();
        tools.add(tool("acquire",
                "Acquire a connection from an endpoint's pool, blocking when saturated.",
                schema(prop("endpoint", "string"), prop("endpoint_id", "string"))));
        tools.add(tool("close-endpoint",
                "Close an endpoint's connection pool and refuse further acquires.",
                schema(prop("endpoint", "string"), prop("endpoint_id", "string"))));
        tools.add(tool("close-read",
                "Release a chunked read handle.",
                schema(prop("handle", "string"), prop("handle_id", "string"))));
        tools.add(tool("close-write",
                "Finalize a chunked write handle, flushing buffered bytes.",
                schema(prop("handle", "string"), prop("handle_id", "string"))));
        tools.add(tool("create-dir",
                "Create a remote directory and any missing parents.",
                pathOpSchema()));
        tools.add(tool("delete-dir",
                "Remove an empty remote directory.",
                pathOpSchema()));
        tools.add(tool("delete-file",
                "Remove a remote regular file.",
                pathOpSchema()));
        tools.add(tool("list-dir",
                "List the immediate children of a remote directory.",
                pathOpSchema()));
        tools.add(tool("open-endpoint",
                "Open or look up a per-(user,host) endpoint and return a handle.",
                openEndpointSchema()));
        tools.add(tool("open-read",
                "Open a remote file for chunked reading and return a handle.",
                pathOpSchema()));
        tools.add(tool("open-write",
                "Open a remote file for chunked writing; creates missing parents.",
                pathOpSchema()));
        tools.add(tool("read",
                "Read up to max_bytes from a read handle; signals EOF when exhausted.",
                readSchema()));
        tools.add(tool("release",
                "Return a connection to its pool's idle set.",
                schema(prop("connection", "string"), prop("connection_id", "string"))));
        tools.add(tool("rename",
                "Same-filesystem rename of a remote path.",
                renameSchema()));
        tools.add(tool("set-mod-time",
                "Set the modification time of a remote file or directory.",
                setModSchema()));
        tools.add(tool("stat",
                "Return mod_time, byte_size, is_dir for a remote path.",
                pathOpSchema()));
        tools.add(tool("write",
                "Append a base64-encoded byte chunk to a write handle.",
                writeSchema()));
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("tools", tools);
        return result;
    }

    private static Map<String, Object> tool(String name, String description, Map<String, Object> inputSchema) {
        Map<String, Object> m = new TreeMap<>();
        m.put("name", name);
        m.put("description", description);
        m.put("inputSchema", inputSchema);
        m.put("outputSchema", anyObject());
        return m;
    }

    private static Map<String, Object> anyObject() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("type", "object");
        m.put("additionalProperties", true);
        return m;
    }

    private static Map<String, Object> prop(String name, String type) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("__name__", name);
        p.put("type", type);
        return p;
    }

    @SafeVarargs
    private static Map<String, Object> schema(Map<String, Object>... props) {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("type", "object");
        Map<String, Object> properties = new LinkedHashMap<>();
        for (Map<String, Object> p : props) {
            String n = (String) p.remove("__name__");
            properties.put(n, p);
        }
        s.put("properties", properties);
        s.put("additionalProperties", true);
        return s;
    }

    private static Map<String, Object> pathOpSchema() {
        return schema(
                prop("connection", "string"),
                prop("connection_id", "string"),
                prop("path", "string"));
    }

    private static Map<String, Object> openEndpointSchema() {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("type", "object");
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("user", Map.of("type", "string"));
        p.put("host", Map.of("type", "string"));
        p.put("port", Map.of("type", "integer"));
        p.put("password", Map.of("type", "string"));
        p.put("settings", Map.of("type", "object"));
        p.put("mc", Map.of("type", "integer"));
        p.put("ct", Map.of("type", "integer"));
        p.put("ka", Map.of("type", "integer"));
        s.put("properties", p);
        s.put("required", List.of("user", "host"));
        s.put("additionalProperties", true);
        return s;
    }

    private static Map<String, Object> readSchema() {
        return schema(
                prop("handle", "string"),
                prop("handle_id", "string"),
                prop("max_bytes", "integer"));
    }

    private static Map<String, Object> writeSchema() {
        return schema(
                prop("handle", "string"),
                prop("handle_id", "string"),
                prop("bytes", "string"),
                prop("data", "string"));
    }

    private static Map<String, Object> renameSchema() {
        return schema(
                prop("connection", "string"),
                prop("connection_id", "string"),
                prop("src", "string"),
                prop("dst", "string"));
    }

    private static Map<String, Object> setModSchema() {
        return schema(
                prop("connection", "string"),
                prop("connection_id", "string"),
                prop("path", "string"),
                prop("time", "integer"),
                prop("mod_time", "integer"));
    }

    // --- dispatch ----------------------------------------------------------

    /** Result envelope. status == "ok" → JSON-RPC result; status == "error" → JSON-RPC error. */
    public static final class Outcome {
        public final boolean isError;
        public final int errorCode;
        public final String errorMessage;
        public final Map<String, Object> result;

        Outcome(Map<String, Object> result) {
            this.isError = false;
            this.errorCode = 0;
            this.errorMessage = null;
            this.result = result;
        }

        Outcome(int code, String msg) {
            this.isError = true;
            this.errorCode = code;
            this.errorMessage = msg;
            this.result = null;
        }
    }

    public static Outcome dispatch(Transport t, String name, Map<String, Object> args) {
        String canonical = name == null ? "" : name.replace('_', '-');
        try {
            return switch (canonical) {
                case "open-endpoint" -> openEndpoint(t, args);
                case "close-endpoint" -> closeEndpoint(t, args);
                case "acquire" -> acquire(t, args);
                case "release" -> release(t, args);
                case "list-dir" -> listDir(t, args);
                case "stat" -> stat(t, args);
                case "open-read" -> openRead(t, args);
                case "read" -> read(t, args);
                case "close-read" -> closeRead(t, args);
                case "open-write" -> openWrite(t, args);
                case "write" -> write(t, args);
                case "close-write" -> closeWrite(t, args);
                case "rename" -> rename(t, args);
                case "delete-file" -> deleteFile(t, args);
                case "delete-dir" -> deleteDir(t, args);
                case "create-dir" -> createDir(t, args);
                case "set-mod-time" -> setModTime(t, args);
                default -> new Outcome(-32601, "method not found: " + name);
            };
        } catch (IOException e) {
            return new Outcome(-32000, "I/O error: " + e.getMessage());
        } catch (RuntimeException e) {
            return new Outcome(-32603, "internal error: " + e.getMessage());
        }
    }

    private static Outcome openEndpoint(Transport t, Map<String, Object> args) {
        String user = str(args, "user");
        String host = str(args, "host");
        if (user == null || host == null) return new Outcome(-32602, "invalid argument: user and host are required");
        Integer port = optInt(args, "port");
        String password = optStr(args, "password");
        Map<String, Object> settings = mapVal(args.get("settings"));
        int mc = pickInt(args, settings, "mc", 5);
        int ct = pickInt(args, settings, "ct", 30);
        int ka = pickInt(args, settings, "ka", 60);
        String epId = t.openEndpoint(user, host, port, password, mc, ct, ka);
        return new Outcome(wrapWithContent(
                kv("endpoint", epId, "endpoint_id", epId, "id", epId, "handle", epId)));
    }

    private static Outcome closeEndpoint(Transport t, Map<String, Object> args) {
        String ep = anyStr(args, "endpoint", "endpoint_id", "id", "handle");
        if (ep == null) return new Outcome(-32602, "invalid argument: endpoint is required");
        t.closeEndpoint(ep);
        return new Outcome(wrapWithContent(new LinkedHashMap<>()));
    }

    private static Outcome acquire(Transport t, Map<String, Object> args) throws IOException {
        String ep = anyStr(args, "endpoint", "endpoint_id", "id", "handle");
        if (ep == null) return new Outcome(-32602, "invalid argument: endpoint is required");
        String connId = t.acquire(ep);
        return new Outcome(wrapWithContent(
                kv("connection", connId, "connection_id", connId, "id", connId, "handle", connId)));
    }

    private static Outcome release(Transport t, Map<String, Object> args) {
        String conn = anyStr(args, "connection", "connection_id", "id", "handle");
        if (conn == null) return new Outcome(-32602, "invalid argument: connection is required");
        t.release(conn);
        return new Outcome(wrapWithContent(new LinkedHashMap<>()));
    }

    private static Outcome listDir(Transport t, Map<String, Object> args) throws IOException {
        String conn = anyStr(args, "connection", "connection_id", "id");
        String path = str(args, "path");
        if (conn == null || path == null) return new Outcome(-32602, "invalid argument");
        try {
            List<Map<String, Object>> entries = t.listDir(conn, path);
            Map<String, Object> r = new LinkedHashMap<>();
            r.put("entries", entries);
            return new Outcome(wrapWithContent(r));
        } catch (SftpFailureException e) {
            return new Outcome(failureBody(e.failure.code()));
        }
    }

    private static Outcome stat(Transport t, Map<String, Object> args) throws IOException {
        String conn = anyStr(args, "connection", "connection_id", "id");
        String path = str(args, "path");
        if (conn == null || path == null) return new Outcome(-32602, "invalid argument");
        try {
            SshSftp.StatResult r = t.stat(conn, path);
            Map<String, Object> body = new LinkedHashMap<>();
            body.put("mod_time", r.modTime());
            body.put("byte_size", r.byteSize());
            body.put("is_dir", r.isDir());
            return new Outcome(wrapWithContent(body));
        } catch (SftpFailureException e) {
            return new Outcome(failureBody(e.failure.code()));
        }
    }

    private static Outcome openRead(Transport t, Map<String, Object> args) throws IOException {
        String conn = anyStr(args, "connection", "connection_id", "id");
        String path = str(args, "path");
        if (conn == null || path == null) return new Outcome(-32602, "invalid argument");
        try {
            String h = t.openRead(conn, path);
            return new Outcome(wrapWithContent(
                    kv("handle", h, "handle_id", h, "read_handle", h, "id", h)));
        } catch (SftpFailureException e) {
            return new Outcome(failureBody(e.failure.code()));
        }
    }

    private static Outcome read(Transport t, Map<String, Object> args) throws IOException {
        String h = anyStr(args, "handle", "handle_id", "read_handle", "id");
        Integer max = optInt(args, "max_bytes");
        if (h == null || max == null) return new Outcome(-32602, "invalid argument");
        byte[] data = t.read(h, max);
        boolean eof = t.atEof(h);
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("bytes", Base64.getEncoder().encodeToString(data));
        body.put("data", Base64.getEncoder().encodeToString(data));
        body.put("eof", eof);
        body.put("EOF", eof);
        return new Outcome(wrapWithContent(body));
    }

    private static Outcome closeRead(Transport t, Map<String, Object> args) {
        String h = anyStr(args, "handle", "handle_id", "read_handle", "id");
        if (h == null) return new Outcome(-32602, "invalid argument");
        t.closeRead(h);
        return new Outcome(wrapWithContent(new LinkedHashMap<>()));
    }

    private static Outcome openWrite(Transport t, Map<String, Object> args) throws IOException {
        String conn = anyStr(args, "connection", "connection_id", "id");
        String path = str(args, "path");
        if (conn == null || path == null) return new Outcome(-32602, "invalid argument");
        try {
            String h = t.openWrite(conn, path);
            return new Outcome(wrapWithContent(
                    kv("handle", h, "handle_id", h, "write_handle", h, "id", h)));
        } catch (SftpFailureException e) {
            return new Outcome(failureBody(e.failure.code()));
        }
    }

    private static Outcome write(Transport t, Map<String, Object> args) throws IOException {
        String h = anyStr(args, "handle", "handle_id", "write_handle", "id");
        String b64 = anyStr(args, "data", "bytes");
        if (h == null || b64 == null) return new Outcome(-32602, "invalid argument");
        byte[] bytes;
        try { bytes = Base64.getDecoder().decode(b64); }
        catch (IllegalArgumentException e) { return new Outcome(-32602, "invalid argument: bytes must be base64"); }
        t.write(h, bytes);
        return new Outcome(wrapWithContent(new LinkedHashMap<>()));
    }

    private static Outcome closeWrite(Transport t, Map<String, Object> args) throws IOException {
        String h = anyStr(args, "handle", "handle_id", "write_handle", "id");
        if (h == null) return new Outcome(-32602, "invalid argument");
        try {
            t.closeWrite(h);
            return new Outcome(wrapWithContent(new LinkedHashMap<>()));
        } catch (SftpFailureException e) {
            return new Outcome(failureBody(e.failure.code()));
        }
    }

    private static Outcome rename(Transport t, Map<String, Object> args) throws IOException {
        String conn = anyStr(args, "connection", "connection_id", "id");
        String src = str(args, "src");
        String dst = str(args, "dst");
        if (conn == null || src == null || dst == null) return new Outcome(-32602, "invalid argument");
        try {
            t.rename(conn, src, dst);
            return new Outcome(wrapWithContent(new LinkedHashMap<>()));
        } catch (SftpFailureException e) {
            return new Outcome(failureBody(e.failure.code()));
        }
    }

    private static Outcome deleteFile(Transport t, Map<String, Object> args) throws IOException {
        String conn = anyStr(args, "connection", "connection_id", "id");
        String path = str(args, "path");
        if (conn == null || path == null) return new Outcome(-32602, "invalid argument");
        try {
            t.deleteFile(conn, path);
            return new Outcome(wrapWithContent(new LinkedHashMap<>()));
        } catch (SftpFailureException e) {
            return new Outcome(failureBody(e.failure.code()));
        }
    }

    private static Outcome deleteDir(Transport t, Map<String, Object> args) throws IOException {
        String conn = anyStr(args, "connection", "connection_id", "id");
        String path = str(args, "path");
        if (conn == null || path == null) return new Outcome(-32602, "invalid argument");
        try {
            t.deleteDir(conn, path);
            return new Outcome(wrapWithContent(new LinkedHashMap<>()));
        } catch (SftpFailureException e) {
            return new Outcome(failureBody(e.failure.code()));
        }
    }

    private static Outcome createDir(Transport t, Map<String, Object> args) throws IOException {
        String conn = anyStr(args, "connection", "connection_id", "id");
        String path = str(args, "path");
        if (conn == null || path == null) return new Outcome(-32602, "invalid argument");
        try {
            t.createDir(conn, path);
            return new Outcome(wrapWithContent(new LinkedHashMap<>()));
        } catch (SftpFailureException e) {
            return new Outcome(failureBody(e.failure.code()));
        }
    }

    private static Outcome setModTime(Transport t, Map<String, Object> args) throws IOException {
        String conn = anyStr(args, "connection", "connection_id", "id");
        String path = str(args, "path");
        Long time = optLong(args, "time");
        if (time == null) time = optLong(args, "mod_time");
        if (conn == null || path == null || time == null) return new Outcome(-32602, "invalid argument");
        try {
            t.setModTime(conn, path, time);
            return new Outcome(wrapWithContent(new LinkedHashMap<>()));
        } catch (SftpFailureException e) {
            return new Outcome(failureBody(e.failure.code()));
        }
    }

    // --- helpers ----------------------------------------------------------

    private static Map<String, Object> failureBody(String code) {
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("error", code);
        body.put(code, true);
        body.put("status", code);
        body.put("type", code);
        return wrapWithContent(body, true);
    }

    private static Map<String, Object> wrapWithContent(Map<String, Object> body) {
        return wrapWithContent(body, false);
    }

    private static Map<String, Object> wrapWithContent(Map<String, Object> body, boolean isError) {
        Map<String, Object> wrapped = new LinkedHashMap<>(body);
        List<Map<String, Object>> content = new ArrayList<>();
        Map<String, Object> textBlock = new LinkedHashMap<>();
        textBlock.put("type", "text");
        textBlock.put("text", Json.stringify(body));
        content.add(textBlock);
        wrapped.put("content", content);
        wrapped.put("isError", isError);
        return wrapped;
    }

    private static Map<String, Object> kv(Object... kvs) {
        Map<String, Object> m = new LinkedHashMap<>();
        for (int i = 0; i + 1 < kvs.length; i += 2) {
            m.put(String.valueOf(kvs[i]), kvs[i + 1]);
        }
        return m;
    }

    private static String str(Map<String, Object> args, String k) {
        Object v = args.get(k);
        return (v instanceof String s) ? s : null;
    }

    private static String optStr(Map<String, Object> args, String k) {
        Object v = args.get(k);
        if (v == null) return null;
        return v.toString();
    }

    private static String anyStr(Map<String, Object> args, String... keys) {
        for (String k : keys) {
            Object v = args.get(k);
            if (v instanceof String s && !s.isEmpty()) return s;
        }
        return null;
    }

    private static Integer optInt(Map<String, Object> args, String k) {
        Object v = args.get(k);
        if (v instanceof Number n) return n.intValue();
        if (v instanceof String s) {
            try { return Integer.parseInt(s); } catch (NumberFormatException ignored) {}
        }
        return null;
    }

    private static Long optLong(Map<String, Object> args, String k) {
        Object v = args.get(k);
        if (v instanceof Number n) return n.longValue();
        if (v instanceof String s) {
            try { return Long.parseLong(s); } catch (NumberFormatException ignored) {}
        }
        return null;
    }

    private static int pickInt(Map<String, Object> args, Map<String, Object> settings, String k, int dflt) {
        Integer v = optInt(args, k);
        if (v != null) return v;
        if (settings != null) {
            Integer sv = optInt(settings, k);
            if (sv != null) return sv;
        }
        return dflt;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> mapVal(Object v) {
        return (v instanceof Map<?, ?> m) ? (Map<String, Object>) m : null;
    }
}
