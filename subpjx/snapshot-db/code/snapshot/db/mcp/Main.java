package snapshot.db.mcp;

import snapshot.db.Json;
import snapshot.db.SnapshotDb;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ConcurrentMap;
import java.util.concurrent.atomic.AtomicLong;

public final class Main {

    private static final ConcurrentMap<String, SnapshotDb> HANDLES = new ConcurrentHashMap<>();
    private static final AtomicLong HANDLE_SEQ = new AtomicLong();

    public static void main(String[] args) throws Exception {
        try {
            Class.forName("org.sqlite.JDBC");
        } catch (ClassNotFoundException ignored) {
            // The xerial driver auto-registers via ServiceLoader on modern JDKs;
            // the explicit load is just a safety net.
        }
        ServerSocket server = new ServerSocket();
        server.bind(new InetSocketAddress(InetAddress.getByName("127.0.0.1"), 0));
        int port = server.getLocalPort();
        System.out.println("MCP_PORT=" + port);
        System.out.flush();
        while (true) {
            Socket sock = server.accept();
            Thread t = new Thread(() -> serveClient(sock));
            t.setDaemon(true);
            t.start();
        }
    }

    private static void serveClient(Socket sock) {
        try (Socket s = sock;
             BufferedReader in = new BufferedReader(
                 new InputStreamReader(s.getInputStream(), StandardCharsets.UTF_8));
             OutputStream out = s.getOutputStream()) {
            String line;
            while ((line = in.readLine()) != null) {
                String response = processRequest(line);
                if (response != null) {
                    out.write((response + "\n").getBytes(StandardCharsets.UTF_8));
                    out.flush();
                }
            }
        } catch (IOException ignored) {
            // client disconnected
        }
    }

    private static String processRequest(String line) {
        Object req;
        try {
            req = Json.parse(line);
        } catch (Exception e) {
            return errorResponse(null, -32700, "Parse error");
        }
        if (!(req instanceof Map)) {
            return errorResponse(null, -32600, "Invalid request");
        }
        @SuppressWarnings("unchecked")
        Map<String, Object> r = (Map<String, Object>) req;
        Object id = r.get("id");
        Object methodObj = r.get("method");
        if (!(methodObj instanceof String method)) {
            return errorResponse(id, -32600, "Invalid request");
        }
        if (id == null) return null; // notification

        try {
            switch (method) {
                case "tools/list":
                    return toolsList(id);
                case "tools/call":
                    return toolsCall(id, r.get("params"));
                default:
                    return errorResponse(id, -32601, "method not found: " + method);
            }
        } catch (Exception e) {
            return errorResponse(id, -32603, "Internal error: " + e.getMessage());
        }
    }

    private static String toolsList(Object id) {
        List<Object> tools = Tools.listTools();
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("tools", tools);
        return successResponse(id, result);
    }

    @SuppressWarnings("unchecked")
    private static String toolsCall(Object id, Object params) {
        if (!(params instanceof Map)) {
            return errorResponse(id, -32602, "Invalid params");
        }
        Map<String, Object> p = (Map<String, Object>) params;
        Object nameObj = p.get("name");
        if (!(nameObj instanceof String name)) {
            return errorResponse(id, -32602, "Invalid params: name is required");
        }
        Object argsObj = p.get("arguments");
        Map<String, Object> args = (argsObj instanceof Map)
            ? (Map<String, Object>) argsObj
            : new LinkedHashMap<>();
        try {
            Map<String, Object> result = Tools.call(name, args, HANDLES, HANDLE_SEQ);
            return successResponse(id, result);
        } catch (Tools.ToolError te) {
            return errorResponse(id, -32000, te.getMessage());
        } catch (Exception e) {
            return errorResponse(id, -32000,
                e.getClass().getSimpleName() + ": " + String.valueOf(e.getMessage()));
        }
    }

    private static String successResponse(Object id, Map<String, Object> result) {
        Map<String, Object> resp = new LinkedHashMap<>();
        resp.put("jsonrpc", "2.0");
        resp.put("id", id);
        resp.put("result", result);
        return Json.stringify(resp);
    }

    private static String errorResponse(Object id, int code, String message) {
        Map<String, Object> err = new LinkedHashMap<>();
        err.put("code", code);
        err.put("message", message);
        Map<String, Object> resp = new LinkedHashMap<>();
        resp.put("jsonrpc", "2.0");
        resp.put("id", id);
        resp.put("error", err);
        return Json.stringify(resp);
    }
}
