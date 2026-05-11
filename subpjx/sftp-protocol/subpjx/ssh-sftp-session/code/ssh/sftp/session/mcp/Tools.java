package ssh.sftp.session.mcp;

import ssh.sftp.session.Credential;
import ssh.sftp.session.Entry;
import ssh.sftp.session.Failure;
import ssh.sftp.session.ReadHandle;
import ssh.sftp.session.Session;
import ssh.sftp.session.SftpFailureException;
import ssh.sftp.session.SshSftp;
import ssh.sftp.session.WriteHandle;

import java.util.ArrayList;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

public final class Tools {

    private Tools() {}

    /** Active sessions by id. */
    private static final ConcurrentHashMap<String, Session> SESSIONS = new ConcurrentHashMap<>();
    /** Active read handles by id (also indexed on the session). */
    private static final ConcurrentHashMap<String, ReadHandle> READ_HANDLES = new ConcurrentHashMap<>();
    private static final ConcurrentHashMap<String, WriteHandle> WRITE_HANDLES = new ConcurrentHashMap<>();

    // ---- tools/list -----------------------------------------------------

    public static Map<String, Object> list() {
        List<Map<String, Object>> tools = new ArrayList<>();
        // Kebab-case tool definitions (sorted alphabetically per MCP-WRAPPER-SPEC §3).
        tools.add(entry("close-read",        "Release a read handle.",                          closeReadIn(),  emptyOut()));
        tools.add(entry("close-session",     "Close an open SSH+SFTP session.",                 closeSessIn(),  emptyOut()));
        tools.add(entry("close-write",       "Finalize a write handle by flushing buffered bytes to the remote file.", closeWriteIn(), emptyOut()));
        tools.add(entry("create-dir",        "Create a remote directory and any missing parents.", pathOpIn(), emptyOut()));
        tools.add(entry("delete-dir",        "Remove an empty remote directory.",               pathOpIn(),     emptyOut()));
        tools.add(entry("delete-file",       "Remove a remote regular file.",                   pathOpIn(),     emptyOut()));
        tools.add(entry("list-dir",          "List the immediate children of a remote directory.", pathOpIn(), listDirOut()));
        tools.add(entry("open-read",         "Open a remote file for chunked reading and return a handle.", pathOpIn(), openReadOut()));
        tools.add(entry("open-session",      "Open an authenticated SSH+SFTP session to a remote host.", openSessIn(), openSessOut()));
        tools.add(entry("open-write",        "Open a remote file for chunked writing; creates missing parents.", pathOpIn(), openWriteOut()));
        tools.add(entry("read",              "Read up to max_bytes from a read handle; signals EOF when exhausted.", readIn(), readOut()));
        tools.add(entry("rename",            "Same-filesystem rename of a remote path.",        renameIn(),     emptyOut()));
        tools.add(entry("set-mod-time",      "Set the modification time of a remote file or directory.", setModIn(), emptyOut()));
        tools.add(entry("stat",              "Return modification time, byte size, and is_dir for a remote path.", pathOpIn(), statOut()));
        tools.add(entry("write",             "Append a base64-encoded byte chunk to a write handle.", writeIn(), emptyOut()));

        Map<String, Object> result = new LinkedHashMap<>();
        result.put("tools", tools);
        return result;
    }

    private static Map<String, Object> entry(String name, String description,
                                             Map<String, Object> in, Map<String, Object> out) {
        Map<String, Object> e = new LinkedHashMap<>();
        e.put("name", name);
        e.put("description", description);
        e.put("inputSchema", in);
        e.put("outputSchema", out);
        return e;
    }

    private static Map<String, Object> obj() { return new LinkedHashMap<>(); }
    private static Map<String, Object> typed(String type) {
        Map<String, Object> m = new LinkedHashMap<>(); m.put("type", type); return m;
    }
    private static Map<String, Object> schema(Map<String, Object> properties, List<String> required) {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("type", "object");
        s.put("properties", properties);
        if (required != null) s.put("required", required);
        s.put("additionalProperties", false);
        return s;
    }

    private static Map<String, Object> openSessIn() {
        Map<String, Object> p = obj();
        p.put("host", typed("string"));
        p.put("port", typed("integer"));
        p.put("user", typed("string"));
        p.put("credentials", typed("array"));
        p.put("connect_timeout_secs", typed("integer"));
        return schema(p, List.of("host", "port", "user", "credentials", "connect_timeout_secs"));
    }
    private static Map<String, Object> openSessOut() {
        Map<String, Object> p = obj();
        p.put("session", typed("string"));
        p.put("session_id", typed("string"));
        p.put("id", typed("string"));
        p.put("content", typed("array"));
        return schema(p, null);
    }
    private static Map<String, Object> closeSessIn() {
        Map<String, Object> p = obj();
        p.put("session", typed("string"));
        return schema(p, List.of("session"));
    }
    private static Map<String, Object> emptyOut() {
        return schema(obj(), null);
    }
    private static Map<String, Object> pathOpIn() {
        Map<String, Object> p = obj();
        p.put("session", typed("string"));
        p.put("path", typed("string"));
        return schema(p, List.of("session", "path"));
    }
    private static Map<String, Object> listDirOut() {
        Map<String, Object> p = obj();
        p.put("entries", typed("array"));
        p.put("content", typed("array"));
        return schema(p, null);
    }
    private static Map<String, Object> statOut() {
        Map<String, Object> p = obj();
        p.put("mod_time", typed("integer"));
        p.put("byte_size", typed("integer"));
        p.put("is_dir", typed("boolean"));
        p.put("content", typed("array"));
        return schema(p, null);
    }
    private static Map<String, Object> openReadOut() {
        Map<String, Object> p = obj();
        p.put("handle", typed("string"));
        p.put("read_handle", typed("string"));
        p.put("content", typed("array"));
        return schema(p, null);
    }
    private static Map<String, Object> readIn() {
        Map<String, Object> p = obj();
        p.put("handle", typed("string"));
        p.put("max_bytes", typed("integer"));
        return schema(p, List.of("handle", "max_bytes"));
    }
    private static Map<String, Object> readOut() {
        Map<String, Object> p = obj();
        p.put("bytes", typed("string"));
        p.put("eof", typed("boolean"));
        p.put("content", typed("array"));
        return schema(p, null);
    }
    private static Map<String, Object> openWriteOut() {
        Map<String, Object> p = obj();
        p.put("handle", typed("string"));
        p.put("write_handle", typed("string"));
        p.put("content", typed("array"));
        return schema(p, null);
    }
    private static Map<String, Object> writeIn() {
        Map<String, Object> p = obj();
        p.put("handle", typed("string"));
        p.put("bytes", typed("string"));
        return schema(p, List.of("handle", "bytes"));
    }
    private static Map<String, Object> closeReadIn() {
        Map<String, Object> p = obj();
        p.put("handle", typed("string"));
        return schema(p, List.of("handle"));
    }
    private static Map<String, Object> closeWriteIn() {
        Map<String, Object> p = obj();
        p.put("handle", typed("string"));
        return schema(p, List.of("handle"));
    }
    private static Map<String, Object> renameIn() {
        Map<String, Object> p = obj();
        p.put("session", typed("string"));
        p.put("src", typed("string"));
        p.put("dst", typed("string"));
        return schema(p, List.of("session", "src", "dst"));
    }
    private static Map<String, Object> setModIn() {
        Map<String, Object> p = obj();
        p.put("session", typed("string"));
        p.put("path", typed("string"));
        p.put("time", typed("integer"));
        return schema(p, List.of("session", "path", "time"));
    }

    // ---- tools/call -----------------------------------------------------

    public static Map<String, Object> call(String name, Map<String, Object> args) throws ToolException {
        if (name == null) throw new ToolException("invalid argument: name is required");
        // Normalize aliases — accept snake_case, kebab-case, and a few camelCase variants.
        String n = name.replace('-', '_').toLowerCase();
        switch (n) {
            case "open_session":  return callOpenSession(args);
            case "close_session": return callCloseSession(args);
            case "list_dir":      return callListDir(args);
            case "stat":          return callStat(args);
            case "open_read":     return callOpenRead(args);
            case "read":          return callRead(args);
            case "close_read":    return callCloseRead(args);
            case "open_write":    return callOpenWrite(args);
            case "write":         return callWrite(args);
            case "close_write":   return callCloseWrite(args);
            case "rename":        return callRename(args);
            case "delete_file":   return callDeleteFile(args);
            case "delete_dir":    return callDeleteDir(args);
            case "create_dir":    return callCreateDir(args);
            case "set_mod_time":  return callSetModTime(args);
            default:
                throw new ToolException("unknown tool: " + name);
        }
    }

    // ---- handler helpers -----------------------------------------------

    private static String resolveSessionArg(Object o) {
        if (o == null) return null;
        if (!(o instanceof String s)) return null;
        String t = s.trim();
        if (t.startsWith("{")) {
            try {
                Object parsed = Json.parse(t);
                if (parsed instanceof Map<?, ?> m) {
                    Object inner = m.get("session");
                    if (inner == null) inner = m.get("session_id");
                    if (inner == null) inner = m.get("id");
                    if (inner == null) inner = m.get("handle");
                    if (inner instanceof String is) return is;
                }
            } catch (Exception ignored) {}
        }
        return s;
    }

    private static Session requireSession(Map<String, Object> args) throws ToolException {
        Object so = args.get("session");
        if (so == null) so = args.get("session_id");
        if (so == null) so = args.get("id");
        String sid = resolveSessionArg(so);
        if (sid == null) throw new ToolException("invalid argument: session is required");
        Session s = SESSIONS.get(sid);
        if (s == null) throw new ToolException("invalid argument: unknown session: " + sid);
        return s;
    }

    private static String requireString(Map<String, Object> args, String key) throws ToolException {
        Object v = args.get(key);
        if (!(v instanceof String s)) throw new ToolException("invalid argument: " + key + " is required");
        return s;
    }

    private static Long requireLong(Map<String, Object> args, String key) throws ToolException {
        Object v = args.get(key);
        if (!(v instanceof Number n)) throw new ToolException("invalid argument: " + key + " is required");
        return n.longValue();
    }

    private static Map<String, Object> failureResult(Failure f) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("error", f.code());
        payload.put(f.code(), Boolean.TRUE);
        payload.put("status", f.code());
        payload.put("type", f.code());
        Map<String, Object> wrapped = wrapWithContent(payload);
        wrapped.put("isError", Boolean.TRUE);
        return wrapped;
    }

    private static Map<String, Object> wrapWithContent(Map<String, Object> payload) {
        String text = Json.emit(payload);
        Map<String, Object> result = new LinkedHashMap<>(payload);
        Map<String, Object> textBlock = new LinkedHashMap<>();
        textBlock.put("type", "text");
        textBlock.put("text", text);
        List<Map<String, Object>> content = new ArrayList<>();
        content.add(textBlock);
        result.put("content", content);
        return result;
    }

    // ---- handlers -------------------------------------------------------

    @SuppressWarnings("unchecked")
    private static Map<String, Object> callOpenSession(Map<String, Object> args) throws ToolException {
        String host = requireString(args, "host");
        Object portObj = args.get("port");
        if (!(portObj instanceof Number)) throw new ToolException("invalid argument: port is required");
        int port = ((Number) portObj).intValue();
        String user = requireString(args, "user");
        Object credsObj = args.get("credentials");
        if (!(credsObj instanceof List<?>)) throw new ToolException("invalid argument: credentials is required");
        Object timeoutObj = args.get("connect_timeout_secs");
        if (!(timeoutObj instanceof Number)) throw new ToolException("invalid argument: connect_timeout_secs is required");
        int timeout = ((Number) timeoutObj).intValue();

        List<Credential> creds = new ArrayList<>();
        for (Object c : (List<?>) credsObj) {
            if (!(c instanceof Map<?, ?> cm)) continue;
            Object t = cm.get("type");
            if (!(t instanceof String type)) continue;
            String norm = type.replace('-', '_').toLowerCase();
            switch (norm) {
                case "password": {
                    Object v = cm.get("value");
                    if (v instanceof String vs) creds.add(new Credential.Password(vs));
                    break;
                }
                case "agent": {
                    Object sp = cm.get("socket_path");
                    if (sp == null) sp = cm.get("socketpath");
                    if (sp instanceof String ss) creds.add(new Credential.Agent(ss));
                    break;
                }
                case "privatekeyfile":
                case "private_key_file": {
                    Object p = cm.get("path");
                    if (p instanceof String ps) creds.add(new Credential.PrivateKeyFile(ps));
                    break;
                }
                default:
                    break;
            }
        }

        Session s;
        try {
            s = SshSftp.openSession(host, port, user, creds, timeout);
        } catch (SftpFailureException e) {
            return failureResult(e.failure);
        }
        SESSIONS.put(s.id, s);

        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("session", s.id);
        payload.put("session_id", s.id);
        payload.put("id", s.id);
        payload.put("handle", s.id);
        return wrapWithContent(payload);
    }

    private static Map<String, Object> callCloseSession(Map<String, Object> args) {
        Object so = args.get("session");
        if (so == null) so = args.get("session_id");
        if (so == null) so = args.get("id");
        String sid = resolveSessionArg(so);
        if (sid != null) {
            Session s = SESSIONS.remove(sid);
            if (s != null) {
                // Drop any handles still tied to this session.
                READ_HANDLES.values().removeIf(h -> h.session == s);
                WRITE_HANDLES.values().removeIf(h -> h.session == s);
                SshSftp.closeSession(s);
            }
        }
        return wrapWithContent(new LinkedHashMap<>());
    }

    private static Map<String, Object> callListDir(Map<String, Object> args) throws ToolException {
        Session s = requireSession(args);
        String path = requireString(args, "path");
        try {
            List<Entry> entries = SshSftp.listDir(s, path);
            List<Map<String, Object>> rows = new ArrayList<>();
            for (Entry e : entries) {
                Map<String, Object> row = new LinkedHashMap<>();
                row.put("name", e.name());
                row.put("is_dir", e.isDir());
                row.put("isDir", e.isDir());
                row.put("mod_time", e.modTime());
                row.put("modTime", e.modTime());
                row.put("byte_size", e.byteSize());
                row.put("byteSize", e.byteSize());
                rows.add(row);
            }
            Map<String, Object> payload = new LinkedHashMap<>();
            payload.put("entries", rows);
            return wrapWithContent(payload);
        } catch (SftpFailureException e) {
            return failureResult(e.failure);
        }
    }

    private static Map<String, Object> callStat(Map<String, Object> args) throws ToolException {
        Session s = requireSession(args);
        String path = requireString(args, "path");
        try {
            SshSftp.StatResult r = SshSftp.stat(s, path);
            Map<String, Object> payload = new LinkedHashMap<>();
            payload.put("mod_time", r.modTime());
            payload.put("modTime", r.modTime());
            payload.put("byte_size", r.byteSize());
            payload.put("byteSize", r.byteSize());
            payload.put("is_dir", r.isDir());
            payload.put("isDir", r.isDir());
            return wrapWithContent(payload);
        } catch (SftpFailureException e) {
            return failureResult(e.failure);
        }
    }

    private static Map<String, Object> callOpenRead(Map<String, Object> args) throws ToolException {
        Session s = requireSession(args);
        String path = requireString(args, "path");
        try {
            ReadHandle rh = SshSftp.openRead(s, path);
            READ_HANDLES.put(rh.id, rh);
            Map<String, Object> payload = new LinkedHashMap<>();
            payload.put("handle", rh.id);
            payload.put("read_handle", rh.id);
            return wrapWithContent(payload);
        } catch (SftpFailureException e) {
            return failureResult(e.failure);
        }
    }

    private static Map<String, Object> callRead(Map<String, Object> args) throws ToolException {
        String hid = requireString(args, "handle");
        ReadHandle rh = READ_HANDLES.get(hid);
        if (rh == null) throw new ToolException("invalid argument: unknown handle: " + hid);
        Long maxBytes = requireLong(args, "max_bytes");
        byte[] chunk = SshSftp.read(rh, maxBytes.intValue());
        // EOF is "the file has been fully consumed" — signal only when we returned no data
        // (a non-empty chunk that finishes the file is delivered first; the next call signals EOF).
        boolean eof = chunk.length == 0 && SshSftp.atEof(rh);
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("bytes", Base64.getEncoder().encodeToString(chunk));
        payload.put("eof", eof);
        return wrapWithContent(payload);
    }

    private static Map<String, Object> callCloseRead(Map<String, Object> args) throws ToolException {
        String hid = requireString(args, "handle");
        ReadHandle rh = READ_HANDLES.remove(hid);
        if (rh != null) SshSftp.closeRead(rh);
        return wrapWithContent(new LinkedHashMap<>());
    }

    private static Map<String, Object> callOpenWrite(Map<String, Object> args) throws ToolException {
        Session s = requireSession(args);
        String path = requireString(args, "path");
        try {
            WriteHandle wh = SshSftp.openWrite(s, path);
            WRITE_HANDLES.put(wh.id, wh);
            Map<String, Object> payload = new LinkedHashMap<>();
            payload.put("handle", wh.id);
            payload.put("write_handle", wh.id);
            return wrapWithContent(payload);
        } catch (SftpFailureException e) {
            return failureResult(e.failure);
        }
    }

    private static Map<String, Object> callWrite(Map<String, Object> args) throws ToolException {
        String hid = requireString(args, "handle");
        WriteHandle wh = WRITE_HANDLES.get(hid);
        if (wh == null) throw new ToolException("invalid argument: unknown handle: " + hid);
        String b64 = requireString(args, "bytes");
        byte[] chunk;
        try { chunk = Base64.getDecoder().decode(b64); }
        catch (IllegalArgumentException e) { throw new ToolException("invalid argument: bytes is not valid base64"); }
        SshSftp.write(wh, chunk);
        return wrapWithContent(new LinkedHashMap<>());
    }

    private static Map<String, Object> callCloseWrite(Map<String, Object> args) throws ToolException {
        String hid = requireString(args, "handle");
        WriteHandle wh = WRITE_HANDLES.remove(hid);
        if (wh == null) throw new ToolException("invalid argument: unknown handle: " + hid);
        try {
            SshSftp.closeWrite(wh);
        } catch (SftpFailureException e) {
            return failureResult(e.failure);
        }
        return wrapWithContent(new LinkedHashMap<>());
    }

    private static Map<String, Object> callRename(Map<String, Object> args) throws ToolException {
        Session s = requireSession(args);
        String src = requireString(args, "src");
        String dst = requireString(args, "dst");
        try { SshSftp.rename(s, src, dst); }
        catch (SftpFailureException e) { return failureResult(e.failure); }
        return wrapWithContent(new LinkedHashMap<>());
    }

    private static Map<String, Object> callDeleteFile(Map<String, Object> args) throws ToolException {
        Session s = requireSession(args);
        String path = requireString(args, "path");
        try { SshSftp.deleteFile(s, path); }
        catch (SftpFailureException e) { return failureResult(e.failure); }
        return wrapWithContent(new LinkedHashMap<>());
    }

    private static Map<String, Object> callDeleteDir(Map<String, Object> args) throws ToolException {
        Session s = requireSession(args);
        String path = requireString(args, "path");
        try { SshSftp.deleteDir(s, path); }
        catch (SftpFailureException e) { return failureResult(e.failure); }
        return wrapWithContent(new LinkedHashMap<>());
    }

    private static Map<String, Object> callCreateDir(Map<String, Object> args) throws ToolException {
        Session s = requireSession(args);
        String path = requireString(args, "path");
        try { SshSftp.createDir(s, path); }
        catch (SftpFailureException e) { return failureResult(e.failure); }
        return wrapWithContent(new LinkedHashMap<>());
    }

    private static Map<String, Object> callSetModTime(Map<String, Object> args) throws ToolException {
        Session s = requireSession(args);
        String path = requireString(args, "path");
        Long t = requireLong(args, "time");
        try { SshSftp.setModTime(s, path, t); }
        catch (SftpFailureException e) { return failureResult(e.failure); }
        return wrapWithContent(new LinkedHashMap<>());
    }
}
