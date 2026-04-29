package kitchensync.mcp;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import kitchensync.KitchenSync;

import java.io.*;
import java.net.*;

public class McpServer {
    private static final ObjectMapper MAPPER = new ObjectMapper();

    public static void start() throws Exception {
        ServerSocket server = new ServerSocket();
        server.bind(new InetSocketAddress("127.0.0.1", 0));
        int port = server.getLocalPort();
        System.out.println("MCP_PORT=" + port);
        System.out.flush();

        Runtime.getRuntime().addShutdownHook(new Thread(() -> {
            try { server.close(); } catch (Exception ignored) {}
        }));

        while (true) {
            Socket conn = server.accept();
            handleConnection(conn);
        }
    }

    private static void handleConnection(Socket conn) throws Exception {
        try (conn) {
            BufferedReader reader = new BufferedReader(new InputStreamReader(conn.getInputStream(), "UTF-8"));
            PrintWriter writer = new PrintWriter(new OutputStreamWriter(conn.getOutputStream(), "UTF-8"), true);
            String line;
            while ((line = reader.readLine()) != null) {
                String response = handleRequest(line.trim());
                if (response != null) {
                    writer.println(response);
                }
            }
        }
    }

    private static String handleRequest(String json) {
        if (json.isEmpty()) return null;
        JsonNode req;
        try {
            req = MAPPER.readTree(json);
        } catch (Exception e) {
            return null;
        }

        String method = req.path("method").asText("");
        JsonNode id = req.get("id");
        boolean hasId = id != null && !id.isNull();

        try {
            return switch (method) {
                case "initialize" -> hasId ? success(id, buildInitializeResult()) : null;
                case "notifications/initialized" -> null;
                case "tools/list" -> hasId ? success(id, buildToolsListResult()) : null;
                case "tools/call" -> hasId ? handleToolsCall(id, req.path("params")) : null;
                default -> hasId ? error(id, -32601, "Method not found") : null;
            };
        } catch (Exception e) {
            return hasId ? error(id, -32603, e.getClass().getName() + ": " + e.getMessage()) : null;
        }
    }

    private static ObjectNode buildInitializeResult() {
        ObjectNode result = MAPPER.createObjectNode();
        result.put("protocolVersion", "2025-03-26");
        result.putObject("capabilities").putObject("tools");
        ObjectNode serverInfo = result.putObject("serverInfo");
        serverInfo.put("name", "kitchensync");
        serverInfo.put("version", "0");
        return result;
    }

    private static ObjectNode buildToolsListResult() {
        ObjectNode result = MAPPER.createObjectNode();
        ArrayNode tools = result.putArray("tools");

        ObjectNode runTool = tools.addObject();
        runTool.put("name", "run");
        runTool.put("description", "Run kitchensync with the given arguments");
        ObjectNode schema = runTool.putObject("inputSchema");
        schema.put("type", "object");
        ObjectNode props = schema.putObject("properties");
        ObjectNode argsProp = props.putObject("args");
        argsProp.put("type", "array");
        argsProp.putObject("items").put("type", "string");
        argsProp.put("description", "Command-line arguments");
        schema.putArray("required").add("args");
        schema.put("additionalProperties", false);

        return result;
    }

    private static String handleToolsCall(JsonNode id, JsonNode params) throws Exception {
        String name = params.path("name").asText("");
        JsonNode arguments = params.path("arguments");

        if (!"run".equals(name)) {
            return error(id, -32602, "unknown tool: " + name);
        }

        if (!arguments.has("args")) {
            return error(id, -32602, "args is required");
        }
        JsonNode argsNode = arguments.get("args");
        if (!argsNode.isArray()) {
            return error(id, -32602, "args must be an array");
        }

        String[] args = new String[argsNode.size()];
        for (int i = 0; i < argsNode.size(); i++) {
            args[i] = argsNode.get(i).asText();
        }

        try {
            int result = KitchenSync.run(args);
            return toolSuccess(id, String.valueOf(result));
        } catch (UnsupportedOperationException e) {
            return toolNotImpl(id);
        } catch (Exception e) {
            return error(id, -32603, e.getClass().getName() + ": " + e.getMessage());
        }
    }

    private static String success(JsonNode id, JsonNode result) throws Exception {
        ObjectNode response = MAPPER.createObjectNode();
        response.put("jsonrpc", "2.0");
        response.set("id", id);
        response.set("result", result);
        return MAPPER.writeValueAsString(response);
    }

    private static String toolSuccess(JsonNode id, String text) throws Exception {
        ObjectNode result = MAPPER.createObjectNode();
        ArrayNode content = result.putArray("content");
        ObjectNode item = content.addObject();
        item.put("type", "text");
        item.put("text", text);
        result.put("isError", false);
        return success(id, result);
    }

    private static String toolNotImpl(JsonNode id) throws Exception {
        ObjectNode result = MAPPER.createObjectNode();
        ArrayNode content = result.putArray("content");
        ObjectNode item = content.addObject();
        item.put("type", "text");
        item.put("text", "not yet implemented");
        result.put("isError", true);
        return success(id, result);
    }

    private static String error(JsonNode id, int code, String message) {
        try {
            ObjectNode response = MAPPER.createObjectNode();
            response.put("jsonrpc", "2.0");
            if (id != null) response.set("id", id);
            ObjectNode err = response.putObject("error");
            err.put("code", code);
            err.put("message", message);
            return MAPPER.writeValueAsString(response);
        } catch (Exception e) {
            return "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32603,\"message\":\"serialization error\"}}";
        }
    }
}
