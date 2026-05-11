package sftp.protocol.mcp;

import sftp.protocol.Transport;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.Map;

public final class Main {
    private Main() {}

    public static void main(String[] args) throws Exception {
        ServerSocket server = new ServerSocket(0, 128, InetAddress.getByName("127.0.0.1"));
        int port = server.getLocalPort();
        System.out.println("MCP_PORT=" + port);
        System.out.flush();

        Transport transport = new Transport();

        while (true) {
            Socket sock;
            try {
                sock = server.accept();
            } catch (IOException e) {
                break;
            }
            Thread.startVirtualThread(() -> handle(sock, transport));
        }
    }

    private static void handle(Socket sock, Transport transport) {
        try (Socket s = sock;
             BufferedReader br = new BufferedReader(
                     new InputStreamReader(s.getInputStream(), StandardCharsets.UTF_8));
             OutputStream out = s.getOutputStream()) {
            String line;
            while ((line = br.readLine()) != null) {
                if (line.isEmpty()) continue;
                String resp = handleRequest(transport, line);
                if (resp != null) {
                    out.write(resp.getBytes(StandardCharsets.UTF_8));
                    out.write('\n');
                    out.flush();
                }
            }
        } catch (IOException ignored) {
            // client disconnect
        }
    }

    @SuppressWarnings("unchecked")
    static String handleRequest(Transport transport, String line) {
        Object id = null;
        try {
            Object parsed = Json.parse(line);
            if (!(parsed instanceof Map<?, ?>)) {
                return Json.stringify(errorResp(null, -32600, "invalid request"));
            }
            Map<String, Object> req = (Map<String, Object>) parsed;
            id = req.get("id");
            String method = (String) req.get("method");
            if (method == null) {
                if (id == null) return null;
                return Json.stringify(errorResp(id, -32600, "invalid request"));
            }
            if ("tools/list".equals(method)) {
                return Json.stringify(successResp(id, Tools.list()));
            }
            if ("tools/call".equals(method)) {
                Map<String, Object> params = (Map<String, Object>) req.get("params");
                if (params == null) {
                    if (id == null) return null;
                    return Json.stringify(errorResp(id, -32602, "invalid params"));
                }
                String name = (String) params.get("name");
                Object argsObj = params.get("arguments");
                Map<String, Object> args = (argsObj instanceof Map<?, ?>)
                        ? (Map<String, Object>) argsObj : new LinkedHashMap<>();
                Tools.Outcome out = Tools.dispatch(transport, name, args);
                if (out.isError) {
                    return Json.stringify(errorResp(id, out.errorCode, out.errorMessage));
                }
                return Json.stringify(successResp(id, out.result));
            }
            if (id == null) return null;
            return Json.stringify(errorResp(id, -32601, "method not found: " + method));
        } catch (RuntimeException e) {
            if (id == null) return null;
            return Json.stringify(errorResp(id, -32603, "internal error: " + e.getMessage()));
        }
    }

    private static Map<String, Object> successResp(Object id, Map<String, Object> result) {
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("jsonrpc", "2.0");
        r.put("id", id);
        r.put("result", result);
        return r;
    }

    private static Map<String, Object> errorResp(Object id, int code, String message) {
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("jsonrpc", "2.0");
        r.put("id", id);
        Map<String, Object> err = new LinkedHashMap<>();
        err.put("code", code);
        err.put("message", message);
        r.put("error", err);
        return r;
    }
}
