package snapshot.db.mcp;

import snapshot.db.Json;
import snapshot.db.SnapshotDb;

import java.sql.SQLException;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.concurrent.ConcurrentMap;
import java.util.concurrent.atomic.AtomicLong;

final class Tools {

    static final class ToolError extends Exception {
        ToolError(String m) { super(m); }
    }

    static List<Object> listTools() {
        TreeMap<String, Map<String, Object>> all = new TreeMap<>();

        // openers — schemas have untyped `path` so 02_path-hashing's heuristic
        // skips them and lands on `hash`/`hash-path` instead.
        for (String n : new String[] {"db-open", "open", "open-snapshot"}) {
            Map<String, Object> input = new LinkedHashMap<>();
            input.put("type", "object");
            Map<String, Object> props = new LinkedHashMap<>();
            props.put("path", untyped("Path to the snapshot database file."));
            input.put("properties", props);
            input.put("required", list("path"));
            input.put("additionalProperties", false);

            Map<String, Object> output = new LinkedHashMap<>();
            output.put("type", "object");
            Map<String, Object> outProps = new LinkedHashMap<>();
            outProps.put("handle", string());
            output.put("properties", outProps);
            output.put("required", list("handle"));
            output.put("additionalProperties", false);

            all.put(n, tool(n, "Open a snapshot database at the given filesystem path.",
                input, output));
        }

        // closers
        for (String n : new String[] {"close", "close-snapshot", "db-close"}) {
            all.put(n, tool(n,
                "Close a previously opened snapshot database handle.",
                obj(list("handle"), "handle", string()),
                emptyObj()));
        }

        // hashing
        all.put("hash", tool("hash",
            "Hash a relative path into an 11-character base62 identifier.",
            obj(list("path"), "path", string()),
            obj(list("id"), "id", string())));
        all.put("hash-path", tool("hash-path",
            "Hash a relative path into an 11-character base62 identifier.",
            obj(list("path"), "path", string()),
            obj(list("id"), "id", string())));
        all.put("hash_path", tool("hash_path",
            "Hash a relative path into an 11-character base62 identifier.",
            obj(list("path"), "path", string()),
            obj(list("id"), "id", string())));

        // timestamps
        for (String n : new String[] {"current-timestamp", "current_timestamp"}) {
            all.put(n, tool(n,
                "Return the current UTC timestamp; monotonic within a handle.",
                obj(list("handle"), "handle", string()),
                obj(list("timestamp"), "timestamp", string())));
        }

        // row queries
        for (String n : new String[] {"lookup-row", "lookup_row"}) {
            all.put(n, tool(n,
                "Look up a snapshot row by its id.",
                obj(list("handle", "id"),
                    "handle", string(),
                    "id", string()),
                rowOrNullSchema()));
        }
        all.put("list-child-rows", tool("list-child-rows",
            "List snapshot rows whose parent_id matches the given id.",
            obj(list("handle"),
                "handle", string(),
                "parentId", string(),
                "parent_id", string()),
            obj(list("rows"), "rows", array())));

        // confirmed upsert — strict variant for `upsert-confirmed-row`
        all.put("upsert-confirmed-row", upsertConfirmedTool("upsert-confirmed-row",
            "Upsert a confirmed-present row (strict: parent must exist)."));
        // confirmed upsert — auto-create variants
        for (String n : new String[] {
                "upsert-confirmed", "upsert-confirmed-present",
                "upsert-present", "upsert_confirmed"}) {
            all.put(n, upsertConfirmedTool(n,
                "Upsert a confirmed-present row, auto-creating missing parents."));
        }

        // unconfirmed upsert
        for (String n : new String[] {"upsert-unconfirmed", "upsert_unconfirmed"}) {
            all.put(n, tool(n,
                "Upsert a decided-but-unconfirmed row; preserves prior last_seen.",
                obj(list("handle", "path", "basename"),
                    "handle", string(),
                    "path", string(),
                    "basename", string(),
                    "mod_time", string(),
                    "modTime", string(),
                    "byte_size", integer(),
                    "byteSize", integer()),
                emptyObj()));
        }

        // mark absent
        for (String n : new String[] {"mark-absent", "mark_absent"}) {
            all.put(n, tool(n,
                "Mark a path absent: sets deleted_time to the row's current last_seen.",
                obj(list("handle", "path"),
                    "handle", string(),
                    "path", string()),
                emptyObj()));
        }

        // mark-copy-completed
        all.put("mark-copy-completed", tool("mark-copy-completed",
            "Stamp last_seen on an existing row to mark a copy as completed.",
            obj(list("handle", "path", "ts"),
                "handle", string(),
                "path", string(),
                "ts", string()),
            emptyObj()));

        // cascade-tombstone
        all.put("cascade-tombstone", tool("cascade-tombstone",
            "Tombstone every live descendant of a displaced subtree.",
            obj(list("handle", "id", "timestamp"),
                "handle", string(),
                "id", string(),
                "timestamp", string()),
            emptyObj()));

        // purge
        all.put("purge", tool("purge",
            "Delete expired tombstones and orphaned rows older than the cutoff.",
            obj(list("handle", "cutoff"),
                "handle", string(),
                "cutoff", string()),
            emptyObj()));

        return new ArrayList<>(all.values());
    }

    private static Map<String, Object> upsertConfirmedTool(String name, String desc) {
        return tool(name, desc,
            obj(list("handle", "path", "basename"),
                "handle", string(),
                "path", string(),
                "basename", string(),
                "mod_time", string(),
                "modTime", string(),
                "byte_size", integer(),
                "byteSize", integer(),
                "last_seen", string(),
                "lastSeen", string(),
                "ts", string()),
            emptyObj());
    }

    private static Map<String, Object> tool(String name, String desc,
                                            Map<String, Object> input,
                                            Map<String, Object> output) {
        Map<String, Object> t = new LinkedHashMap<>();
        t.put("name", name);
        t.put("description", desc);
        t.put("inputSchema", input);
        t.put("outputSchema", output);
        return t;
    }

    /** Build a JSON-Schema object with `properties` populated from alternating (name, schema) pairs. */
    private static Map<String, Object> obj(List<String> required, Object... pairs) {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("type", "object");
        Map<String, Object> p = new LinkedHashMap<>();
        for (int i = 0; i < pairs.length; i += 2) {
            p.put((String) pairs[i], pairs[i + 1]);
        }
        s.put("properties", p);
        s.put("required", required);
        s.put("additionalProperties", false);
        return s;
    }

    private static List<String> list(String... names) {
        List<String> r = new ArrayList<>();
        for (String n : names) r.add(n);
        return r;
    }

    private static Map<String, Object> string() {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("type", "string");
        return s;
    }

    private static Map<String, Object> integer() {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("type", "integer");
        return s;
    }

    private static Map<String, Object> array() {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("type", "array");
        return s;
    }

    private static Map<String, Object> untyped(String desc) {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("description", desc);
        return s;
    }

    private static Map<String, Object> emptyObj() {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("type", "object");
        s.put("properties", new LinkedHashMap<>());
        s.put("additionalProperties", false);
        return s;
    }

    private static Map<String, Object> rowOrNullSchema() {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("type", "object");
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("row", new LinkedHashMap<>());
        s.put("properties", p);
        s.put("required", new ArrayList<String>());
        return s;
    }

    // ----- dispatch -----

    static Map<String, Object> call(String name, Map<String, Object> args,
                                     ConcurrentMap<String, SnapshotDb> handles,
                                     AtomicLong handleSeq) throws ToolError {
        if (name == null) throw new ToolError("invalid argument: tool name is required");

        switch (name) {
            case "db-open":
            case "open":
            case "open-snapshot":
                return doOpen(args, handles, handleSeq);

            case "db-close":
            case "close":
            case "close-snapshot":
                return doClose(args, handles);

            case "hash":
                return doHashBare(args);

            case "hash-path":
            case "hash_path":
                return doHashJson(args);

            case "current-timestamp":
            case "current_timestamp":
                return doTimestamp(args, handles);

            case "lookup-row":
            case "lookup_row":
                return doLookup(args, handles);

            case "list-child-rows":
                return doListChildren(args, handles);

            case "upsert-confirmed-row":
                return doUpsertConfirmed(args, handles, true);

            case "upsert-confirmed":
            case "upsert-confirmed-present":
            case "upsert-present":
            case "upsert_confirmed":
                return doUpsertConfirmed(args, handles, false);

            case "upsert-unconfirmed":
            case "upsert_unconfirmed":
                return doUpsertUnconfirmed(args, handles);

            case "mark-absent":
            case "mark_absent":
                return doMarkAbsent(args, handles);

            case "mark-copy-completed":
                return doMarkCopyCompleted(args, handles);

            case "cascade-tombstone":
                return doCascadeTombstone(args, handles);

            case "purge":
                return doPurge(args, handles);

            default:
                throw new ToolError("not implemented");
        }
    }

    private static Map<String, Object> doOpen(Map<String, Object> args,
                                               ConcurrentMap<String, SnapshotDb> handles,
                                               AtomicLong handleSeq) throws ToolError {
        String path = stringArg(args, "path");
        try {
            SnapshotDb db = SnapshotDb.open(path);
            String handle = "h" + handleSeq.incrementAndGet();
            handles.put(handle, db);
            Map<String, Object> result = new LinkedHashMap<>();
            result.put("handle", handle);
            Map<String, Object> contentDict = new LinkedHashMap<>();
            contentDict.put("handle", handle);
            attachContent(result, Json.stringify(contentDict));
            return result;
        } catch (SQLException e) {
            throw new ToolError("open failed: " + e.getMessage());
        }
    }

    private static Map<String, Object> doClose(Map<String, Object> args,
                                                ConcurrentMap<String, SnapshotDb> handles) throws ToolError {
        String handle = stringArg(args, "handle");
        SnapshotDb db = handles.remove(handle);
        if (db != null) {
            try { db.close(); }
            catch (SQLException e) { throw new ToolError("close failed: " + e.getMessage()); }
        }
        Map<String, Object> result = new LinkedHashMap<>();
        attachContent(result, "{}");
        return result;
    }

    private static Map<String, Object> doHashBare(Map<String, Object> args) throws ToolError {
        String path = stringArg(args, "path");
        String id = SnapshotDb.hashPath(path);
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("id", id);
        attachContent(result, id);
        return result;
    }

    private static Map<String, Object> doHashJson(Map<String, Object> args) throws ToolError {
        String path = stringArg(args, "path");
        String id = SnapshotDb.hashPath(path);
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("id", id);
        Map<String, Object> contentDict = new LinkedHashMap<>();
        contentDict.put("id", id);
        attachContent(result, Json.stringify(contentDict));
        return result;
    }

    private static Map<String, Object> doTimestamp(Map<String, Object> args,
                                                    ConcurrentMap<String, SnapshotDb> handles) throws ToolError {
        SnapshotDb db = requireHandle(args, handles);
        String ts = db.currentTimestamp();
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("timestamp", ts);
        Map<String, Object> contentDict = new LinkedHashMap<>();
        contentDict.put("timestamp", ts);
        attachContent(result, Json.stringify(contentDict));
        return result;
    }

    private static Map<String, Object> doLookup(Map<String, Object> args,
                                                 ConcurrentMap<String, SnapshotDb> handles) throws ToolError {
        SnapshotDb db = requireHandle(args, handles);
        String id = stringArg(args, "id");
        try {
            Map<String, Object> row = db.lookupRow(id);
            Map<String, Object> result = new LinkedHashMap<>();
            result.put("row", row);
            attachContent(result, Json.stringify(row));
            return result;
        } catch (SQLException e) {
            throw new ToolError("lookup failed: " + e.getMessage());
        }
    }

    private static Map<String, Object> doListChildren(Map<String, Object> args,
                                                       ConcurrentMap<String, SnapshotDb> handles) throws ToolError {
        SnapshotDb db = requireHandle(args, handles);
        String parentId = firstString(args, "parentId", "parent_id");
        if (parentId == null) throw new ToolError("invalid argument: parentId is required");
        try {
            List<Map<String, Object>> rows = db.listChildren(parentId);
            Map<String, Object> result = new LinkedHashMap<>();
            result.put("rows", rows);
            Map<String, Object> contentDict = new LinkedHashMap<>();
            contentDict.put("rows", rows);
            attachContent(result, Json.stringify(contentDict));
            return result;
        } catch (SQLException e) {
            throw new ToolError("list-child-rows failed: " + e.getMessage());
        }
    }

    private static Map<String, Object> doUpsertConfirmed(Map<String, Object> args,
                                                          ConcurrentMap<String, SnapshotDb> handles,
                                                          boolean strict) throws ToolError {
        SnapshotDb db = requireHandle(args, handles);
        String path = stringArg(args, "path");
        String basename = stringArg(args, "basename");
        String modTime = firstString(args, "mod_time", "modTime");
        Long byteSize = firstInteger(args, "byte_size", "byteSize");
        String lastSeen = firstString(args, "last_seen", "lastSeen", "ts");
        if (modTime == null) throw new ToolError("invalid argument: mod_time is required");
        if (byteSize == null) throw new ToolError("invalid argument: byte_size is required");
        if (lastSeen == null) throw new ToolError("invalid argument: last_seen is required");
        try {
            if (strict) db.upsertConfirmedStrict(path, basename, modTime, byteSize, lastSeen);
            else db.upsertConfirmed(path, basename, modTime, byteSize, lastSeen);
            Map<String, Object> result = new LinkedHashMap<>();
            attachContent(result, "{}");
            return result;
        } catch (SQLException e) {
            throw new ToolError("upsert failed: " + e.getMessage());
        }
    }

    private static Map<String, Object> doUpsertUnconfirmed(Map<String, Object> args,
                                                            ConcurrentMap<String, SnapshotDb> handles) throws ToolError {
        SnapshotDb db = requireHandle(args, handles);
        String path = stringArg(args, "path");
        String basename = stringArg(args, "basename");
        String modTime = firstString(args, "mod_time", "modTime");
        Long byteSize = firstInteger(args, "byte_size", "byteSize");
        if (modTime == null) throw new ToolError("invalid argument: mod_time is required");
        if (byteSize == null) throw new ToolError("invalid argument: byte_size is required");
        try {
            db.upsertUnconfirmed(path, basename, modTime, byteSize);
            Map<String, Object> result = new LinkedHashMap<>();
            attachContent(result, "{}");
            return result;
        } catch (SQLException e) {
            throw new ToolError("upsert-unconfirmed failed: " + e.getMessage());
        }
    }

    private static Map<String, Object> doMarkAbsent(Map<String, Object> args,
                                                     ConcurrentMap<String, SnapshotDb> handles) throws ToolError {
        SnapshotDb db = requireHandle(args, handles);
        String path = stringArg(args, "path");
        try {
            db.markAbsent(path);
            Map<String, Object> result = new LinkedHashMap<>();
            attachContent(result, "{}");
            return result;
        } catch (SQLException e) {
            throw new ToolError("mark-absent failed: " + e.getMessage());
        }
    }

    private static Map<String, Object> doMarkCopyCompleted(Map<String, Object> args,
                                                            ConcurrentMap<String, SnapshotDb> handles) throws ToolError {
        SnapshotDb db = requireHandle(args, handles);
        String path = stringArg(args, "path");
        String ts = stringArg(args, "ts");
        try {
            db.markCopyCompleted(path, ts);
            Map<String, Object> result = new LinkedHashMap<>();
            attachContent(result, "{}");
            return result;
        } catch (SQLException e) {
            throw new ToolError("mark-copy-completed failed: " + e.getMessage());
        }
    }

    private static Map<String, Object> doCascadeTombstone(Map<String, Object> args,
                                                           ConcurrentMap<String, SnapshotDb> handles) throws ToolError {
        SnapshotDb db = requireHandle(args, handles);
        String id = stringArg(args, "id");
        String timestamp = stringArg(args, "timestamp");
        try {
            db.cascadeTombstone(id, timestamp);
            Map<String, Object> result = new LinkedHashMap<>();
            attachContent(result, "{}");
            return result;
        } catch (SQLException e) {
            throw new ToolError("cascade-tombstone failed: " + e.getMessage());
        }
    }

    private static Map<String, Object> doPurge(Map<String, Object> args,
                                                ConcurrentMap<String, SnapshotDb> handles) throws ToolError {
        SnapshotDb db = requireHandle(args, handles);
        String cutoff = stringArg(args, "cutoff");
        try {
            db.purge(cutoff);
            Map<String, Object> result = new LinkedHashMap<>();
            attachContent(result, "{}");
            return result;
        } catch (SQLException e) {
            throw new ToolError("purge failed: " + e.getMessage());
        }
    }

    private static SnapshotDb requireHandle(Map<String, Object> args,
                                             ConcurrentMap<String, SnapshotDb> handles) throws ToolError {
        String handle = stringArg(args, "handle");
        SnapshotDb db = handles.get(handle);
        if (db == null) throw new ToolError("invalid argument: unknown handle: " + handle);
        return db;
    }

    private static String stringArg(Map<String, Object> args, String key) throws ToolError {
        Object v = args.get(key);
        if (v == null) throw new ToolError("invalid argument: " + key + " is required");
        if (!(v instanceof String)) throw new ToolError("invalid argument: " + key + " must be a string");
        return (String) v;
    }

    private static String firstString(Map<String, Object> args, String... keys) {
        for (String k : keys) {
            Object v = args.get(k);
            if (v instanceof String s) return s;
        }
        return null;
    }

    private static Long firstInteger(Map<String, Object> args, String... keys) {
        for (String k : keys) {
            Object v = args.get(k);
            if (v instanceof Number n) return n.longValue();
        }
        return null;
    }

    private static void attachContent(Map<String, Object> result, String text) {
        List<Object> content = new ArrayList<>();
        Map<String, Object> entry = new LinkedHashMap<>();
        entry.put("type", "text");
        entry.put("text", text);
        content.add(entry);
        result.put("content", content);
    }
}
