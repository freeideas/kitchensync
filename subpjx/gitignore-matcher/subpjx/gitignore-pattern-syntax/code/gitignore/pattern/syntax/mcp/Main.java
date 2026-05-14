package gitignore.pattern.syntax.mcp;

import gitignore.pattern.syntax.GitignorePatternSyntax;
import gitignore.pattern.syntax.PatternLine;
import gitignore.pattern.syntax.PatternMatchInput;
import gitignore.pattern.syntax.PatternMatchResult;
import gitignore.pattern.syntax.PatternRule;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.PrintWriter;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.InetAddress;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class Main {
    private Main() {}

    public static void main(String[] args) throws Exception {
        try (ServerSocket server = new ServerSocket(0, 50, InetAddress.getByName("127.0.0.1"))) {
            int port = server.getLocalPort();
            System.out.println("MCP_PORT=" + port);

            while (true) {
                try (Socket socket = server.accept()) {
                    handleConnection(socket);
                }
            }
        }
    }

    private static void handleConnection(Socket socket) throws IOException {
        try (BufferedReader reader = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
             PrintWriter writer = new PrintWriter(socket.getOutputStream(), true, StandardCharsets.UTF_8)) {
            String line;
            while ((line = reader.readLine()) != null) {
                line = line.trim();
                if (line.isEmpty()) {
                    continue;
                }
                String response = handleRequest(line);
                if (response != null) {
                    writer.println(response);
                    writer.flush();
                }
            }
        }
    }

    private static String handleRequest(String line) {
        Object request;
        try {
            request = MiniJson.parse(line);
        } catch (Exception ex) {
            return writeParseError(null, -32700, "parse error");
        }

        if (!(request instanceof Map<?, ?> requestObj)) {
            return writeParseError(null, -32600, "invalid request");
        }

        Object id = requestObj.get("id");
        if (!(id instanceof String) && !(id instanceof Number) && id != null) {
            return writeError(null, -32600, "invalid request", null);
        }

        if (!"2.0".equals(String.valueOf(requestObj.get("jsonrpc")))) {
            return id == null ? null : writeError(id, -32600, "invalid request", null);
        }

        Object methodObj = requestObj.get("method");
        if (!(methodObj instanceof String)) {
            return id == null ? null : writeError(id, -32600, "invalid request", null);
        }
        String method = (String) methodObj;

        if (id == null) {
            return null;
        }

        if ("tools/list".equals(method)) {
            return handleToolsList(id);
        }

        if ("tools/call".equals(method)) {
            return handleToolsCall(id, requestObj.get("params"));
        }

        return writeError(id, -32601, "method not found: " + method, null);
    }

    private static String handleToolsList(Object id) {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("tools", toolsList());
        return writeResult(id, result);
    }

    private static String handleToolsCall(Object id, Object paramsObj) {
        if (!(paramsObj instanceof Map<?, ?> params)) {
            return writeError(id, -32602, "invalid params: params must be object", null);
        }

        Object nameObj = params.get("name");
        Object argsObj = params.get("arguments");
        if (!(nameObj instanceof String)) {
            return writeError(id, -32602, "invalid argument: name is required", null);
        }
        String name = (String) nameObj;

        return switch (name) {
            case "compile-patterns" -> handleCompilePatterns(id, argsObj);
            case "match-patterns" -> handleMatchPatterns(id, argsObj);
            default -> writeError(id, -32000, "not implemented", null);
        };
    }

    private static String handleCompilePatterns(Object id, Object argsObj) {
        try {
            PatternLine[] patternLines = parsePatternLines(argsObj);
            PatternRule[] rules = GitignorePatternSyntax.compile_patterns(patternLines);
            Map<String, Object> result = new LinkedHashMap<>();
            List<Object> list = new ArrayList<>();
            for (PatternRule rule : rules) {
                Map<String, Object> item = new LinkedHashMap<>();
                item.put("pattern", rule.pattern());
                item.put("negated", rule.negated());
                item.put("directory_only", rule.directoryOnly());
                item.put("anchored", rule.anchored());
                item.put("has_slash", rule.hasSlash());
                item.put("regex", rule.regex());
                list.add(item);
            }
            Map<String, Object> rulesMap = new LinkedHashMap<>();
            rulesMap.put("pattern_rules", list);
            return writeResult(id, rulesMap);
        } catch (GitignorePatternSyntax.PatternSyntaxException ex) {
            return writeError(id, -32000, ex.code() + ": " + ex.detail(), null);
        } catch (IllegalArgumentException ex) {
            return writeError(id, -32000, "invalid argument: " + ex.getMessage(), null);
        } catch (Exception ex) {
            return writeError(id, -32603, "internal error", null);
        }
    }

    private static String handleMatchPatterns(Object id, Object argsObj) {
        try {
            if (!(argsObj instanceof Map<?, ?> args)) {
                return writeError(id, -32602, "invalid argument: arguments is required", null);
            }
            Object rulesObj = args.get("pattern_rules");
            Object inputObj = args.get("input");
            if (!(inputObj instanceof Map<?, ?>)) {
                return writeError(id, -32000, "invalid argument: input is required", null);
            }

            PatternRule[] rules = parsePatternRules(rulesObj);
            PatternMatchInput input = parseInput((Map<?, ?>) inputObj);
            PatternMatchResult result = GitignorePatternSyntax.match_patterns(rules, input);
            Map<String, Object> resultMap = new LinkedHashMap<>();
            resultMap.put("matches", result.matches());
            resultMap.put("status", result.status());
            return writeResult(id, resultMap);
        } catch (GitignorePatternSyntax.PatternSyntaxException ex) {
            return writeError(id, -32000, ex.code() + ": " + ex.detail(), null);
        } catch (IllegalArgumentException ex) {
            return writeError(id, -32000, "invalid argument: " + ex.getMessage(), null);
        } catch (Exception ex) {
            return writeError(id, -32603, "internal error", null);
        }
    }

    private static PatternLine[] parsePatternLines(Object argsObj) {
        if (!(argsObj instanceof Map<?, ?> args)) {
            throw new IllegalArgumentException("arguments is required");
        }
        Object patternLinesObj = args.get("pattern_lines");
        if (!(patternLinesObj instanceof List<?> list)) {
            throw new IllegalArgumentException("pattern_lines is required");
        }

        List<PatternLine> lines = new ArrayList<>();
        for (Object item : list) {
            if (item instanceof String value) {
                lines.add(new PatternLine(value));
                continue;
            }
            if (!(item instanceof Map<?, ?> map)) {
                throw new IllegalArgumentException("pattern_lines must contain strings or objects");
            }
            Object textObj = map.get("text");
            if (!(textObj instanceof String)) {
                throw new IllegalArgumentException("pattern line text is required");
            }
            lines.add(new PatternLine((String) textObj));
        }
        return lines.toArray(new PatternLine[0]);
    }

    private static PatternRule[] parsePatternRules(Object rulesObj) {
        if (!(rulesObj instanceof List<?> list)) {
            throw new IllegalArgumentException("pattern_rules is required");
        }
        List<PatternRule> rules = new ArrayList<>();
        for (Object item : list) {
            if (!(item instanceof Map<?, ?> rawRule)) {
                throw new IllegalArgumentException("pattern_rules must contain objects");
            }
            Object patternObj = rawRule.get("pattern");
            if (!(patternObj instanceof String)) {
                throw new IllegalArgumentException("pattern is required");
            }
            String pattern = (String) patternObj;
            boolean negated = boolValue(rawRule, "negated", "negated");
            boolean directoryOnly = boolValue(rawRule, "directoryOnly", "directory_only");
            boolean anchored = boolValue(rawRule, "anchored", "anchored");
            boolean hasSlash = boolValue(rawRule, "hasSlash", "has_slash");
            String regex = null;
            Object regexObj = rawRule.get("regex");
            if (regexObj instanceof String) {
                regex = (String) regexObj;
            }
            rules.add(new PatternRule(pattern, negated, directoryOnly, anchored, hasSlash, regex));
        }
        return rules.toArray(new PatternRule[0]);
    }

    private static PatternMatchInput parseInput(Map<?, ?> input) {
        Object pathObj = input.get("path");
        if (!(pathObj instanceof String)) {
            throw new IllegalArgumentException("path is required");
        }
        Boolean directory = boolOrNull(input.get("is_directory"));
        if (directory == null) {
            directory = boolOrNull(input.get("isDirectory"));
            if (directory == null) {
                throw new IllegalArgumentException("is_directory is required");
            }
        }
        return new PatternMatchInput((String) pathObj, directory);
    }

    private static Boolean boolOrNull(Object value) {
        if (value instanceof Boolean bool) {
            return bool;
        }
        return null;
    }

    private static boolean boolValue(Map<?, ?> rawRule, String key, String keySnake) {
        Object value = rawRule.get(key);
        if (value == null) {
            value = rawRule.get(keySnake);
        }
        if (!(value instanceof Boolean bool)) {
            throw new IllegalArgumentException(key + " is required");
        }
        return bool;
    }

    private static List<Map<String, Object>> toolsList() {
        List<Map<String, Object>> tools = new ArrayList<>();
        tools.add(tool("compile-patterns",
                "Compile gitignore pattern lines into reusable pattern rules.",
                compileInputSchema(),
                compileOutputSchema()));
        tools.add(tool("match-patterns",
                "Match compiled pattern rules against a path input.",
                matchInputSchema(),
                matchOutputSchema()));
        tools.sort((a, b) -> ((String) a.get("name")).compareTo((String) b.get("name")));
        return tools;
    }

    private static Map<String, Object> tool(String name, String description, Map<String, Object> inputSchema,
                                            Map<String, Object> outputSchema) {
        Map<String, Object> tool = new LinkedHashMap<>();
        tool.put("name", name);
        tool.put("description", description);
        tool.put("inputSchema", inputSchema);
        tool.put("outputSchema", outputSchema);
        return tool;
    }

    private static Map<String, Object> compileInputSchema() {
        Map<String, Object> properties = new LinkedHashMap<>();
        properties.put("pattern_lines", listOf(textSchema()));

        Map<String, Object> schema = new LinkedHashMap<>();
        schema.put("type", "object");
        schema.put("properties", properties);
        schema.put("required", List.of("pattern_lines"));
        schema.put("additionalProperties", false);
        return schema;
    }

    private static Map<String, Object> compileOutputSchema() {
        Map<String, Object> patternRule = patternRuleSchema();
        Map<String, Object> properties = new LinkedHashMap<>();
        properties.put("pattern_rules", listOf(patternRule));

        Map<String, Object> schema = new LinkedHashMap<>();
        schema.put("type", "object");
        schema.put("properties", properties);
        schema.put("required", List.of("pattern_rules"));
        schema.put("additionalProperties", false);
        return schema;
    }

    private static Map<String, Object> matchInputSchema() {
        Map<String, Object> patternRule = patternRuleSchema();
        Map<String, Object> input = patternMatchInputSchema();

        Map<String, Object> properties = new LinkedHashMap<>();
        properties.put("pattern_rules", listOf(patternRule));
        properties.put("input", input);

        Map<String, Object> schema = new LinkedHashMap<>();
        schema.put("type", "object");
        schema.put("properties", properties);
        schema.put("required", List.of("pattern_rules", "input"));
        schema.put("additionalProperties", false);
        return schema;
    }

    private static Map<String, Object> matchOutputSchema() {
        Map<String, Object> status = new LinkedHashMap<>();
        status.put("type", "string");
        status.put("enum", List.of(PatternMatchResult.INCLUDED, PatternMatchResult.IGNORED));

        Map<String, Object> properties = new LinkedHashMap<>();
        properties.put("matches", boolSchema());
        properties.put("status", status);

        Map<String, Object> schema = new LinkedHashMap<>();
        schema.put("type", "object");
        schema.put("properties", properties);
        schema.put("required", List.of("matches", "status"));
        schema.put("additionalProperties", false);
        return schema;
    }

    private static Map<String, Object> patternRuleSchema() {
        Map<String, Object> properties = new LinkedHashMap<>();
        properties.put("pattern", textSchema());
        properties.put("negated", boolSchema());
        properties.put("directory_only", boolSchema());
        properties.put("anchored", boolSchema());
        properties.put("has_slash", boolSchema());
        properties.put("regex", textSchema());

        Map<String, Object> schema = new LinkedHashMap<>();
        schema.put("type", "object");
        schema.put("properties", properties);
        schema.put("required", List.of("pattern", "negated", "directory_only", "anchored", "has_slash", "regex"));
        schema.put("additionalProperties", false);
        return schema;
    }

    private static Map<String, Object> patternMatchInputSchema() {
        Map<String, Object> properties = new LinkedHashMap<>();
        properties.put("path", textSchema());
        properties.put("is_directory", boolSchema());

        Map<String, Object> schema = new LinkedHashMap<>();
        schema.put("type", "object");
        schema.put("properties", properties);
        schema.put("required", List.of("path", "is_directory"));
        schema.put("additionalProperties", false);
        return schema;
    }

    private static Map<String, Object> listOf(Object itemSchema) {
        Map<String, Object> list = new LinkedHashMap<>();
        list.put("type", "array");
        list.put("items", itemSchema);
        return list;
    }

    private static Map<String, Object> textSchema() {
        Map<String, Object> schema = new LinkedHashMap<>();
        schema.put("type", "string");
        return schema;
    }

    private static Map<String, Object> boolSchema() {
        Map<String, Object> schema = new LinkedHashMap<>();
        schema.put("type", "boolean");
        return schema;
    }

    private static String writeResult(Object id, Object result) {
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("jsonrpc", "2.0");
        response.put("id", id);
        response.put("result", result);
        return MiniJson.stringify(response);
    }

    private static String writeError(Object id, long code, String message, Object data) {
        Map<String, Object> error = new LinkedHashMap<>();
        error.put("code", code);
        error.put("message", message);
        if (data != null) {
            error.put("data", data);
        }
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("jsonrpc", "2.0");
        response.put("id", id);
        response.put("error", error);
        return MiniJson.stringify(response);
    }

    private static String writeParseError(Object id, int code, String message) {
        return writeError(id, code, message, null);
    }
}
