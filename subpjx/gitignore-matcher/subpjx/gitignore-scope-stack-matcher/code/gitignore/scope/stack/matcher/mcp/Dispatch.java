package gitignore.scope.stack.matcher.mcp;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Parses incoming JSON-RPC requests, dispatches to tools, returns serialized responses. */
final class Dispatch {
    private Dispatch() {}

    static String handleRequest(String line) {
        Object parsed;
        try {
            parsed = Json.parse(line);
        } catch (RuntimeException e) {
            return errorResponse(null, -32700, "parse error: " + e.getMessage());
        }
        if (!(parsed instanceof Map<?, ?> req)) {
            return errorResponse(null, -32600, "invalid request");
        }
        Object id = req.get("id");
        Object methodObj = req.get("method");
        if (!(methodObj instanceof String method)) {
            return errorResponse(id, -32600, "invalid request");
        }

        try {
            if ("tools/list".equals(method)) {
                Map<String, Object> result = new LinkedHashMap<>();
                result.put("tools", Tools.list());
                return successResponse(id, result);
            }
            if ("tools/call".equals(method)) {
                Object paramsObj = req.get("params");
                if (!(paramsObj instanceof Map<?, ?> params)) {
                    return errorResponse(id, -32602, "invalid params");
                }
                Object nameObj = params.get("name");
                if (!(nameObj instanceof String name)) {
                    return errorResponse(id, -32602, "invalid params: missing name");
                }
                Object argsObj = params.get("arguments");
                Map<String, Object> argMap = argsObj instanceof Map<?, ?> m ? toStringKeyMap(m) : new LinkedHashMap<>();
                try {
                    Map<String, Object> result = Tools.call(name, argMap);
                    return successResponse(id, result);
                } catch (RuntimeException e) {
                    return errorResponse(id, -32000, e.getMessage());
                }
            }
            return errorResponse(id, -32601, "method not found: " + method);
        } catch (RuntimeException e) {
            return errorResponse(id, -32603, "internal error: " + e.getMessage());
        }
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> toStringKeyMap(Map<?, ?> m) {
        Map<String, Object> out = new LinkedHashMap<>();
        for (Map.Entry<?, ?> e : m.entrySet()) {
            out.put((String) e.getKey(), e.getValue());
        }
        return out;
    }

    private static String successResponse(Object id, Map<String, Object> result) {
        Map<String, Object> resp = new LinkedHashMap<>();
        resp.put("jsonrpc", "2.0");
        resp.put("id", id);
        resp.put("result", result);
        return Json.write(resp);
    }

    private static String errorResponse(Object id, int code, String message) {
        Map<String, Object> err = new LinkedHashMap<>();
        err.put("code", (long) code);
        err.put("message", message == null ? "" : message);
        Map<String, Object> resp = new LinkedHashMap<>();
        resp.put("jsonrpc", "2.0");
        resp.put("id", id);
        resp.put("error", err);
        return Json.write(resp);
    }
}
