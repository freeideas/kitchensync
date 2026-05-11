package rfc8089.file.uri.mcp;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import rfc8089.file.uri.FileUri;
import rfc8089.file.uri.FileUriException;

public final class Main {

    public static void main(String[] args) throws Exception {
        ServerSocket server = new ServerSocket(0, 50, InetAddress.getByName("127.0.0.1"));
        int port = server.getLocalPort();
        System.out.println("MCP_PORT=" + port);
        System.out.flush();
        while (true) {
            Socket sock = server.accept();
            Thread t = new Thread(() -> handle(sock));
            t.setDaemon(true);
            t.start();
        }
    }

    private static void handle(Socket sock) {
        try (BufferedReader in = new BufferedReader(
                new InputStreamReader(sock.getInputStream(), StandardCharsets.UTF_8));
             BufferedWriter out = new BufferedWriter(
                new OutputStreamWriter(sock.getOutputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = in.readLine()) != null) {
                if (line.isBlank()) continue;
                String response = handleLine(line);
                if (response != null) {
                    out.write(response);
                    out.write("\n");
                    out.flush();
                }
            }
        } catch (IOException ignored) {
        } finally {
            try { sock.close(); } catch (IOException ignored) {}
        }
    }

    @SuppressWarnings("unchecked")
    private static String handleLine(String line) {
        Map<String, Object> req;
        try {
            Object parsed = Json.parse(line);
            if (!(parsed instanceof Map)) {
                return errorResponse(null, -32600, "invalid request", null);
            }
            req = (Map<String, Object>) parsed;
        } catch (Exception e) {
            return errorResponse(null, -32700, "parse error", null);
        }
        Object id = req.get("id");
        Object methodObj = req.get("method");
        if (!(methodObj instanceof String)) {
            return errorResponse(id, -32600, "invalid request", null);
        }
        String method = (String) methodObj;
        if (id == null) return null; // notification

        if ("tools/list".equals(method)) {
            return toolsList(id);
        }
        if ("tools/call".equals(method)) {
            return toolsCall(id, req.get("params"));
        }
        return errorResponse(id, -32601, "method not found: " + method, null);
    }

    private static String toolsList(Object id) {
        List<Map<String, Object>> tools = new ArrayList<>();
        tools.add(toolFileUriToPath());
        tools.add(toolIsFileUri());
        tools.add(toolLooksLikeBarePath());
        tools.add(toolPathToFileUri());
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("tools", tools);
        return successResponse(id, result);
    }

    private static Map<String, Object> toolIsFileUri() {
        Map<String, Object> tool = new LinkedHashMap<>();
        tool.put("name", "is-file-uri");
        tool.put("description", "True if the string begins with the file: scheme (case-insensitive).");
        Map<String, Object> inProps = new LinkedHashMap<>();
        inProps.put("s", stringSchema());
        tool.put("inputSchema", schema(inProps, List.of("s")));
        Map<String, Object> outProps = new LinkedHashMap<>();
        outProps.put("result", boolSchema());
        tool.put("outputSchema", schema(outProps, List.of("result")));
        return tool;
    }

    private static Map<String, Object> toolLooksLikeBarePath() {
        Map<String, Object> tool = new LinkedHashMap<>();
        tool.put("name", "looks-like-bare-path");
        tool.put("description", "True if the string does not begin with a recognised URI scheme.");
        Map<String, Object> inProps = new LinkedHashMap<>();
        inProps.put("s", stringSchema());
        tool.put("inputSchema", schema(inProps, List.of("s")));
        Map<String, Object> outProps = new LinkedHashMap<>();
        outProps.put("result", boolSchema());
        tool.put("outputSchema", schema(outProps, List.of("result")));
        return tool;
    }

    private static Map<String, Object> toolPathToFileUri() {
        Map<String, Object> tool = new LinkedHashMap<>();
        tool.put("name", "path-to-file-uri");
        tool.put("description", "Produce the file:// URI form of a filesystem path per RFC 8089.");
        Map<String, Object> inProps = new LinkedHashMap<>();
        inProps.put("path", stringSchema());
        inProps.put("cwd", stringSchema());
        tool.put("inputSchema", schema(inProps, List.of("path", "cwd")));
        Map<String, Object> outProps = new LinkedHashMap<>();
        outProps.put("result", stringSchema());
        tool.put("outputSchema", schema(outProps, List.of("result")));
        return tool;
    }

    private static Map<String, Object> toolFileUriToPath() {
        Map<String, Object> tool = new LinkedHashMap<>();
        tool.put("name", "file-uri-to-path");
        tool.put("description", "Extract the filesystem path from a file:// URI per RFC 8089.");
        Map<String, Object> inProps = new LinkedHashMap<>();
        inProps.put("uri", stringSchema());
        inProps.put("style", stringSchema());
        tool.put("inputSchema", schema(inProps, List.of("uri")));
        Map<String, Object> outProps = new LinkedHashMap<>();
        outProps.put("result", stringSchema());
        tool.put("outputSchema", schema(outProps, List.of("result")));
        return tool;
    }

    private static Map<String, Object> stringSchema() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("type", "string");
        return m;
    }

    private static Map<String, Object> boolSchema() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("type", "boolean");
        return m;
    }

    private static Map<String, Object> schema(Map<String, Object> props, List<String> required) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("type", "object");
        m.put("properties", props);
        m.put("required", new ArrayList<>(required));
        m.put("additionalProperties", Boolean.FALSE);
        return m;
    }

    @SuppressWarnings("unchecked")
    private static String toolsCall(Object id, Object paramsObj) {
        if (!(paramsObj instanceof Map)) {
            return errorResponse(id, -32602, "invalid params", null);
        }
        Map<String, Object> params = (Map<String, Object>) paramsObj;
        Object nameObj = params.get("name");
        Object argsObj = params.get("arguments");
        if (!(nameObj instanceof String)) {
            return errorResponse(id, -32602, "invalid params: name is required", null);
        }
        Map<String, Object> args;
        if (argsObj instanceof Map) {
            args = (Map<String, Object>) argsObj;
        } else if (argsObj == null) {
            args = new LinkedHashMap<>();
        } else {
            return errorResponse(id, -32602, "invalid params: arguments must be object", null);
        }
        String name = (String) nameObj;

        switch (name) {
            case "is-file-uri": {
                String s = asString(args.get("s"));
                if (s == null) return errorResponse(id, -32000, "invalid argument: s is required", null);
                Map<String, Object> r = new LinkedHashMap<>();
                r.put("result", FileUri.isFileUri(s));
                return successResponse(id, r);
            }
            case "looks-like-bare-path": {
                String s = asString(args.get("s"));
                if (s == null) return errorResponse(id, -32000, "invalid argument: s is required", null);
                Map<String, Object> r = new LinkedHashMap<>();
                r.put("result", FileUri.looksLikeBarePath(s));
                return successResponse(id, r);
            }
            case "path-to-file-uri": {
                String path = asString(args.get("path"));
                String cwd = asString(args.get("cwd"));
                if (path == null) return errorResponse(id, -32000, "invalid argument: path is required", null);
                if (cwd == null) return errorResponse(id, -32000, "invalid argument: cwd is required", null);
                try {
                    String result = FileUri.pathToFileUri(path, cwd);
                    Map<String, Object> r = new LinkedHashMap<>();
                    r.put("result", result);
                    return successResponse(id, r);
                } catch (FileUriException e) {
                    return errorWithOffset(id, e);
                }
            }
            case "file-uri-to-path":
            case "file_uri_to_path": {
                String uri = asString(args.get("uri"));
                String style = asString(args.get("style"));
                if (uri == null) return errorResponse(id, -32000, "invalid argument: uri is required", null);
                try {
                    String result = FileUri.fileUriToPath(uri, style);
                    Map<String, Object> r = new LinkedHashMap<>();
                    r.put("result", result);
                    List<Map<String, Object>> content = new ArrayList<>();
                    Map<String, Object> c = new LinkedHashMap<>();
                    c.put("type", "text");
                    c.put("text", result);
                    content.add(c);
                    r.put("content", content);
                    return successResponse(id, r);
                } catch (FileUriException e) {
                    return errorWithOffset(id, e);
                }
            }
            default:
                return errorResponse(id, -32000, "not implemented", null);
        }
    }

    private static String errorWithOffset(Object id, FileUriException e) {
        Map<String, Object> data = new LinkedHashMap<>();
        if (e.offset != null) data.put("offset", e.offset);
        String msg = e.getMessage();
        if (msg == null) msg = "file uri error";
        return errorResponse(id, -32000, msg, data.isEmpty() ? null : data);
    }

    private static String asString(Object v) {
        return v instanceof String ? (String) v : null;
    }

    private static String successResponse(Object id, Object result) {
        Map<String, Object> resp = new LinkedHashMap<>();
        resp.put("jsonrpc", "2.0");
        resp.put("id", id);
        resp.put("result", result);
        return Json.stringify(resp);
    }

    private static String errorResponse(Object id, int code, String message, Map<String, Object> data) {
        Map<String, Object> err = new LinkedHashMap<>();
        err.put("code", code);
        err.put("message", message);
        if (data != null && !data.isEmpty()) err.put("data", data);
        Map<String, Object> resp = new LinkedHashMap<>();
        resp.put("jsonrpc", "2.0");
        resp.put("id", id);
        resp.put("error", err);
        return Json.stringify(resp);
    }
}
