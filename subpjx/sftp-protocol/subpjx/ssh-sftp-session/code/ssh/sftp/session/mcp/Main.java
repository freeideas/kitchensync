package ssh.sftp.session.mcp;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.Map;

public final class Main {

    private Main() {}

    public static void main(String[] args) throws IOException {
        ServerSocket server = new ServerSocket(0, 50, InetAddress.getByName("127.0.0.1"));
        int port = server.getLocalPort();
        System.out.println("MCP_PORT=" + port);
        System.out.flush();
        while (true) {
            Socket client = server.accept();
            Thread t = new Thread(() -> handle(client));
            t.setDaemon(true);
            t.start();
        }
    }

    private static void handle(Socket s) {
        try (s;
             BufferedReader in = new BufferedReader(new InputStreamReader(s.getInputStream(), StandardCharsets.UTF_8));
             BufferedWriter out = new BufferedWriter(new OutputStreamWriter(s.getOutputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = in.readLine()) != null) {
                String response = handleRequest(line);
                if (response != null) {
                    out.write(response);
                    out.write("\n");
                    out.flush();
                }
            }
        } catch (IOException ignored) {
        }
    }

    @SuppressWarnings("unchecked")
    private static String handleRequest(String line) {
        Object req;
        try {
            req = Json.parse(line);
        } catch (Exception e) {
            return Json.emit(errorResponse(null, -32700, "Parse error: " + e.getMessage()));
        }
        if (!(req instanceof Map<?, ?> m)) {
            return Json.emit(errorResponse(null, -32600, "Invalid request"));
        }
        Object id = m.get("id");
        Object methodObj = m.get("method");
        if (id == null) {
            return null;
        }
        if (!(methodObj instanceof String method)) {
            return Json.emit(errorResponse(id, -32600, "Invalid request"));
        }
        if (method.equals("tools/list")) {
            return Json.emit(successResponse(id, Tools.list()));
        }
        if (method.equals("tools/call")) {
            Object params = m.get("params");
            if (!(params instanceof Map<?, ?> pm)) {
                return Json.emit(errorResponse(id, -32602, "Invalid params"));
            }
            Object nameObj = pm.get("name");
            if (!(nameObj instanceof String name)) {
                return Json.emit(errorResponse(id, -32602, "Invalid params: name is required"));
            }
            Object argsObj = pm.get("arguments");
            Map<String, Object> args;
            if (argsObj instanceof Map<?, ?> am) {
                args = (Map<String, Object>) am;
            } else {
                args = new LinkedHashMap<>();
            }
            try {
                Map<String, Object> result = Tools.call(name, args);
                return Json.emit(successResponse(id, result));
            } catch (ToolException te) {
                return Json.emit(errorResponse(id, -32000, te.getMessage()));
            } catch (Exception e) {
                return Json.emit(errorResponse(id, -32603, "Internal error: " + e.getMessage()));
            }
        }
        return Json.emit(errorResponse(id, -32601, "method not found: " + method));
    }

    private static Map<String, Object> successResponse(Object id, Object result) {
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("jsonrpc", "2.0");
        r.put("id", id);
        r.put("result", result);
        return r;
    }

    private static Map<String, Object> errorResponse(Object id, int code, String message) {
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("jsonrpc", "2.0");
        r.put("id", id);
        Map<String, Object> e = new LinkedHashMap<>();
        e.put("code", code);
        e.put("message", message);
        r.put("error", e);
        return r;
    }
}
