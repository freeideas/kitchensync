package staged.file.transfer.mcp;

import staged.file.transfer.CleanupRequest;
import staged.file.transfer.CopyRequest;
import staged.file.transfer.DisplaceRequest;
import staged.file.transfer.OperationResult;
import staged.file.transfer.StagedFileTransfer;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class Main {
    private static volatile ServerSocket serverSocket;

    private Main() {
    }

    public static void main(String[] args) throws IOException {
        serverSocket = new ServerSocket(0, 50, InetAddress.getByName("127.0.0.1"));
        System.out.println("MCP_PORT=" + serverSocket.getLocalPort());
        System.out.flush();

        ExecutorService executor = Executors.newCachedThreadPool();
        while (!serverSocket.isClosed()) {
            try {
                Socket socket = serverSocket.accept();
                executor.execute(() -> serve(socket));
            } catch (IOException e) {
                if (!serverSocket.isClosed()) {
                    throw e;
                }
            }
        }
        executor.shutdownNow();
    }

    private static void serve(Socket socket) {
        try (socket;
             BufferedReader reader = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
             BufferedWriter writer = new BufferedWriter(new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                Response response = handleLine(line);
                if (response == null) {
                    continue;
                }
                if (response.body() != null) {
                    writer.write(Json.stringify(response.body()));
                    writer.write('\n');
                    writer.flush();
                }
                if (response.shutdown()) {
                    shutdown();
                    return;
                }
            }
        } catch (IOException ignored) {
        }
    }

    @SuppressWarnings("unchecked")
    private static Response handleLine(String line) {
        Object id = null;
        try {
            Object parsed = Json.parse(line);
            if (!(parsed instanceof Map<?, ?> raw)) {
                return response(null, error(-32600, "invalid request"));
            }
            Map<String, Object> request = (Map<String, Object>) raw;
            id = request.get("id");
            Object methodValue = request.get("method");
            if (!"2.0".equals(request.get("jsonrpc")) || !(methodValue instanceof String method)) {
                return response(id, error(-32600, "invalid request"));
            }
            if (!request.containsKey("id")) {
                if ("aitc/shutdown".equals(method)) {
                    return new Response(null, true);
                }
                return null;
            }
            return switch (method) {
                case "tools/list" -> response(id, result(Map.of("tools", tools())));
                case "tools/call" -> callTool(id, request.get("params"));
                case "aitc/shutdown" -> shutdownResponse(id, request.get("params"));
                default -> response(id, error(-32601, "method not found: " + method));
            };
        } catch (IllegalArgumentException e) {
            return response(id, error(-32700, "parse error"));
        } catch (RuntimeException e) {
            return response(id, error(-32603, "internal error"));
        }
    }

    @SuppressWarnings("unchecked")
    private static Response callTool(Object id, Object params) {
        if (!(params instanceof Map<?, ?> rawParams)) {
            return response(id, error(-32602, "invalid params"));
        }
        Map<String, Object> paramsMap = (Map<String, Object>) rawParams;
        if (!(paramsMap.get("name") instanceof String name) || !(paramsMap.get("arguments") instanceof Map<?, ?> rawArgs)) {
            return response(id, error(-32602, "invalid params"));
        }
        Map<String, Object> args = (Map<String, Object>) rawArgs;
        try {
            return switch (name) {
                case "cleanup-expired" -> response(id, result(cleanupExpired(args)));
                case "copy-file" -> response(id, result(copyFile(args)));
                case "displace" -> response(id, result(displace(args)));
                default -> response(id, error(-32000, "not implemented"));
            };
        } catch (IllegalArgumentException e) {
            return response(id, error(-32000, "invalid argument: " + e.getMessage()));
        } catch (RuntimeException e) {
            return response(id, error(-32000, e.getMessage() == null ? "tool failed" : e.getMessage()));
        }
    }

    private static Map<String, Object> copyFile(Map<String, Object> args) {
        String sourceRoot = string(args, "source_root");
        String destinationRoot = string(args, "destination_root");
        LocalFilesystem source = new LocalFilesystem(sourceRoot);
        LocalFilesystem destination = sourceRoot.equals(destinationRoot) ? source : new LocalFilesystem(destinationRoot);
        OperationResult result = StagedFileTransfer.copy_file(new CopyRequest(
                source,
                string(args, "source_path"),
                destination,
                string(args, "destination_path"),
                Instant.parse(string(args, "winning_mod_time")),
                string(args, "staging_timestamp"),
                string(args, "transfer_id"),
                intValue(args, "chunk_size"),
                intValue(args, "channel_capacity")));
        return operationResult(result);
    }

    private static Map<String, Object> displace(Map<String, Object> args) {
        LocalFilesystem filesystem = new LocalFilesystem(string(args, "filesystem_root"));
        OperationResult result = StagedFileTransfer.displace(new DisplaceRequest(
                filesystem,
                string(args, "path"),
                string(args, "staging_timestamp")));
        return operationResult(result);
    }

    private static Map<String, Object> cleanupExpired(Map<String, Object> args) {
        LocalFilesystem filesystem = new LocalFilesystem(string(args, "filesystem_root"));
        OperationResult result = StagedFileTransfer.cleanup_expired(new CleanupRequest(
                filesystem,
                string(args, "directory_path"),
                string(args, "bak_cutoff_exclusive"),
                string(args, "tmp_cutoff_exclusive")));
        return operationResult(result);
    }

    private static Map<String, Object> operationResult(OperationResult result) {
        TreeMap<String, Object> map = new TreeMap<>();
        map.put("status", result.status().name());
        map.put("created_paths", result.created_paths());
        map.put("removed_paths", result.removed_paths());
        if (result.backup_path() != null) {
            map.put("backup_path", result.backup_path());
        }
        if (result.temporary_path() != null) {
            map.put("temporary_path", result.temporary_path());
        }
        if (result.final_path() != null) {
            map.put("final_path", result.final_path());
        }
        if (result.error() != null) {
            map.put("error", result.error().name());
        }
        return map;
    }

    private static Response shutdownResponse(Object id, Object params) {
        if (params != null && !(params instanceof Map<?, ?> map && map.isEmpty())) {
            return response(id, error(-32602, "invalid params"));
        }
        return new Response(ok(id, Map.of()), true);
    }

    private static void shutdown() {
        try {
            serverSocket.close();
        } catch (IOException ignored) {
        }
        System.exit(0);
    }

    private static Map<String, Object> ok(Object id, Map<String, Object> resultValue) {
        return Map.of("jsonrpc", "2.0", "id", id, "result", resultValue);
    }

    private static Map<String, Object> result(Map<String, Object> value) {
        return Map.of("result", value);
    }

    private static Map<String, Object> error(int code, String message) {
        return Map.of("error", Map.of("code", code, "message", message));
    }

    private static Response response(Object id, Map<String, Object> payload) {
        TreeMap<String, Object> body = new TreeMap<>();
        body.put("jsonrpc", "2.0");
        body.put("id", id);
        body.putAll(payload);
        return new Response(body, false);
    }

    private static List<Map<String, Object>> tools() {
        return List.of(
                tool("cleanup-expired", "Remove expired staged transfer metadata directories.", cleanupInput(), operationResultSchema()),
                tool("copy-file", "Copy one file through a staged temporary path.", copyInput(), operationResultSchema()),
                tool("displace", "Move one existing file or directory into a backup path.", displaceInput(), operationResultSchema()));
    }

    private static Map<String, Object> tool(
            String name,
            String description,
            Map<String, Object> inputSchema,
            Map<String, Object> outputSchema) {
        return Map.of(
                "name", name,
                "description", description,
                "inputSchema", inputSchema,
                "outputSchema", outputSchema);
    }

    private static Map<String, Object> copyInput() {
        return objectSchema(Map.of(
                        "source_root", Map.of("type", "string"),
                        "source_path", Map.of("type", "string"),
                        "destination_root", Map.of("type", "string"),
                        "destination_path", Map.of("type", "string"),
                        "winning_mod_time", Map.of("type", "string"),
                        "staging_timestamp", Map.of("type", "string"),
                        "transfer_id", Map.of("type", "string"),
                        "chunk_size", Map.of("type", "integer"),
                        "channel_capacity", Map.of("type", "integer")),
                List.of("source_root", "source_path", "destination_root", "destination_path",
                        "winning_mod_time", "staging_timestamp", "transfer_id", "chunk_size", "channel_capacity"));
    }

    private static Map<String, Object> displaceInput() {
        return objectSchema(Map.of(
                        "filesystem_root", Map.of("type", "string"),
                        "path", Map.of("type", "string"),
                        "staging_timestamp", Map.of("type", "string")),
                List.of("filesystem_root", "path", "staging_timestamp"));
    }

    private static Map<String, Object> cleanupInput() {
        return objectSchema(Map.of(
                        "filesystem_root", Map.of("type", "string"),
                        "directory_path", Map.of("type", "string"),
                        "bak_cutoff_exclusive", Map.of("type", "string"),
                        "tmp_cutoff_exclusive", Map.of("type", "string")),
                List.of("filesystem_root", "directory_path", "bak_cutoff_exclusive", "tmp_cutoff_exclusive"));
    }

    private static Map<String, Object> objectSchema(Map<String, Object> properties, List<String> required) {
        return Map.of(
                "type", "object",
                "properties", properties,
                "required", required,
                "additionalProperties", false);
    }

    private static Map<String, Object> operationResultSchema() {
        return Map.of(
                "type", "object",
                "properties", Map.of(
                        "status", Map.of("type", "string"),
                        "created_paths", Map.of("type", "array", "items", Map.of("type", "string")),
                        "removed_paths", Map.of("type", "array", "items", Map.of("type", "string")),
                        "backup_path", Map.of("type", "string"),
                        "temporary_path", Map.of("type", "string"),
                        "final_path", Map.of("type", "string"),
                        "error", Map.of("type", "string")),
                "required", List.of("status", "created_paths", "removed_paths"),
                "additionalProperties", false);
    }

    private static String string(Map<String, Object> map, String key) {
        Object value = map.get(key);
        if (!(value instanceof String text)) {
            throw new IllegalArgumentException(key + " must be a string");
        }
        return text;
    }

    private static int intValue(Map<String, Object> map, String key) {
        Object value = map.get(key);
        if (!(value instanceof Number number)) {
            throw new IllegalArgumentException(key + " must be an integer");
        }
        return number.intValue();
    }

    private record Response(Map<String, Object> body, boolean shutdown) {
    }
}
