package gitignore.matcher.mcp;

import gitignore.matcher.EntryKind;
import gitignore.matcher.IgnoreMatcher;
import gitignore.matcher.IgnoreMatcherException;
import gitignore.matcher.IgnoreOptions;
import gitignore.matcher.MatchResult;
import gitignore.matcher.PathEntry;
import gitignore.matcher.PatternLayer;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;

public final class Main {
    private static volatile boolean running = true;
    private static ServerSocket serverSocket;
    private static final List<Socket> sockets = new CopyOnWriteArrayList<>();
    private static final ConcurrentHashMap<String, IgnoreMatcher> matchers = new ConcurrentHashMap<>();
    private static final AtomicLong nextMatcherId = new AtomicLong(1L);

    private Main() {}

    public static void main(String[] args) throws IOException {
        try (ServerSocket server = new ServerSocket(0, 50, java.net.InetAddress.getByName("127.0.0.1"))) {
            serverSocket = server;
            System.out.println("MCP_PORT=" + server.getLocalPort());
            System.out.flush();
            while (running) {
                try {
                    Socket socket = server.accept();
                    sockets.add(socket);
                    Thread thread = new Thread(() -> serve(socket));
                    thread.setDaemon(false);
                    thread.start();
                } catch (IOException ex) {
                    if (running) {
                        System.err.println(ex.getMessage());
                    }
                }
            }
        }
    }

    private static void serve(Socket socket) {
        try (socket;
                BufferedReader in = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
                BufferedWriter out = new BufferedWriter(new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = in.readLine()) != null && running) {
                String response = handleLine(line);
                if (response != null) {
                    out.write(response);
                    out.write('\n');
                    out.flush();
                }
            }
        } catch (IOException ignored) {
            // Connection lifecycle is owned by the client.
        } finally {
            sockets.remove(socket);
        }
    }

    @SuppressWarnings("unchecked")
    private static String handleLine(String line) {
        Object id = null;
        try {
            Object parsed = Json.parse(line);
            if (!(parsed instanceof Map<?, ?> rawRequest)) {
                return response(null, error(-32600, "invalid request"));
            }
            Map<String, Object> request = (Map<String, Object>) rawRequest;
            id = request.get("id");
            Object method = request.get("method");
            if (!"2.0".equals(request.get("jsonrpc")) || !(method instanceof String)) {
                return response(id, error(-32600, "invalid request"));
            }
            if (!request.containsKey("id")) {
                if ("aitc/shutdown".equals(method)) {
                    shutdown();
                }
                return null;
            }
            return handleRequest(id, (String) method, request.get("params"));
        } catch (IllegalArgumentException ex) {
            return response(id, error(-32700, "parse error"));
        } catch (Exception ex) {
            return response(id, error(-32603, "internal error"));
        }
    }

    private static String handleRequest(Object id, String method, Object params) {
        return switch (method) {
            case "tools/list" -> response(id, Map.of("result", Schemas.toolsList()));
            case "tools/call" -> callTool(id, params);
            case "aitc/shutdown" -> {
                if (params != null && !(params instanceof Map<?, ?> map && map.isEmpty())) {
                    yield response(id, error(-32602, "invalid params"));
                }
                String response = response(id, Map.of("result", Map.of()));
                shutdownSoon();
                yield response;
            }
            default -> response(id, error(-32601, "method not found: " + method));
        };
    }

    @SuppressWarnings("unchecked")
    private static String callTool(Object id, Object params) {
        if (!(params instanceof Map<?, ?> rawParams)) {
            return response(id, error(-32602, "invalid params"));
        }
        Map<String, Object> paramsMap = (Map<String, Object>) rawParams;
        if (!(paramsMap.get("name") instanceof String name) || !(paramsMap.get("arguments") instanceof Map<?, ?> rawArguments)) {
            return response(id, error(-32602, "invalid params"));
        }
        Map<String, Object> arguments = (Map<String, Object>) rawArguments;
        try {
            return switch (name) {
                case "compile" -> response(id, Map.of("result", compileMatcher(arguments)));
                case "empty" -> response(id, Map.of("result", emptyMatcher(arguments)));
                case "extend" -> response(id, Map.of("result", extendMatcher(arguments)));
                case "match" -> response(id, Map.of("result", matchEntry(arguments)));
                case "filter" -> response(id, Map.of("result", filterEntries(arguments)));
                default -> response(id, error(-32000, "not implemented"));
            };
        } catch (IgnoreMatcherException ex) {
            return response(id, error(-32000, ex.category()));
        } catch (IllegalArgumentException ex) {
            return response(id, error(-32000, "invalid argument: " + ex.getMessage()));
        } catch (Exception ex) {
            return response(id, error(-32000, ex.getMessage() == null ? "tool failed" : ex.getMessage()));
        }
    }

    private static String compileMatcher(Map<String, Object> arguments) {
        requireOnly(arguments, Set.of("layers", "options"));
        return matcherId(IgnoreMatcher.compile(layers(required(arguments, "layers")), options(arguments.get("options"))));
    }

    private static String emptyMatcher(Map<String, Object> arguments) {
        requireOnly(arguments, Set.of("options"));
        return matcherId(IgnoreMatcher.empty(options(arguments.get("options"))));
    }

    private static String extendMatcher(Map<String, Object> arguments) {
        requireOnly(arguments, Set.of("matcher", "layer"));
        IgnoreMatcher m = matcher(required(arguments, "matcher"));
        return matcherId(m.extend(layer(required(arguments, "layer"))));
    }

    private static Map<String, Object> matchEntry(Map<String, Object> arguments) {
        requireOnly(arguments, Set.of("matcher", "entry"));
        IgnoreMatcher m = matcher(required(arguments, "matcher"));
        return matchResult(m.match(pathEntry(required(arguments, "entry"))));
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> filterEntries(Map<String, Object> arguments) {
        requireOnly(arguments, Set.of("matcher", "entries"));
        IgnoreMatcher m = matcher(required(arguments, "matcher"));
        Object value = required(arguments, "entries");
        if (!(value instanceof List<?> rawEntries)) {
            throw new IllegalArgumentException("entries must be an array");
        }
        ArrayList<PathEntry> entries = new ArrayList<>();
        for (Object item : rawEntries) {
            entries.add(pathEntry(item));
        }
        ArrayList<Map<String, Object>> kept = new ArrayList<>();
        for (PathEntry entry : m.filter(entries)) {
            kept.add(pathEntryJson(entry));
        }
        return Map.of("entries", kept);
    }

    private static String matcherId(IgnoreMatcher m) {
        String id = Long.toString(nextMatcherId.getAndIncrement());
        matchers.put(id, m);
        return id;
    }

    private static IgnoreMatcher matcher(Object value) {
        String id = string(value, "matcher");
        IgnoreMatcher m = matchers.get(id);
        if (m == null) {
            throw new IllegalArgumentException("unknown matcher");
        }
        return m;
    }

    private static PatternLayer layer(Object value) {
        if (!(value instanceof Map<?, ?> rawLayer)) {
            throw new IllegalArgumentException("layer must be an object");
        }
        return layerFromMap(rawLayer);
    }

    @SuppressWarnings("unchecked")
    private static List<PatternLayer> layers(Object value) {
        if (!(value instanceof List<?> rawLayers)) {
            throw new IllegalArgumentException("layers must be an array");
        }
        ArrayList<PatternLayer> layers = new ArrayList<>();
        for (Object item : rawLayers) {
            if (!(item instanceof Map<?, ?> rawLayer)) {
                throw new IllegalArgumentException("layer must be an object");
            }
            layers.add(layerFromMap(rawLayer));
        }
        return layers;
    }

    @SuppressWarnings("unchecked")
    private static PatternLayer layerFromMap(Map<?, ?> rawLayer) {
        Map<String, Object> layer = (Map<String, Object>) rawLayer;
        requireOnly(layer, Set.of("base_path", "pattern_text", "source_name"));
        String basePath = string(required(layer, "base_path"), "base_path");
        String patternText = string(required(layer, "pattern_text"), "pattern_text");
        Object sourceName = layer.get("source_name");
        if (sourceName != null && !(sourceName instanceof String)) {
            throw new IllegalArgumentException("source_name must be a string or null");
        }
        return new PatternLayer(basePath, patternText, (String) sourceName);
    }

    @SuppressWarnings("unchecked")
    private static IgnoreOptions options(Object value) {
        IgnoreOptions defaults = IgnoreOptions.defaults();
        if (value == null) {
            return defaults;
        }
        if (!(value instanceof Map<?, ?> rawOptions)) {
            throw new IllegalArgumentException("options must be an object");
        }
        Map<String, Object> options = (Map<String, Object>) rawOptions;
        requireOnly(options, Set.of(
                "always_excluded_directory_names",
                "default_excluded_directory_names",
                "ignore_symlinks",
                "ignore_special_entries"));
        return new IgnoreOptions(
                stringSet(options.getOrDefault("always_excluded_directory_names", defaults.alwaysExcludedDirectoryNames()), "always_excluded_directory_names"),
                stringSet(options.getOrDefault("default_excluded_directory_names", defaults.defaultExcludedDirectoryNames()), "default_excluded_directory_names"),
                bool(options.getOrDefault("ignore_symlinks", defaults.ignoreSymlinks()), "ignore_symlinks"),
                bool(options.getOrDefault("ignore_special_entries", defaults.ignoreSpecialEntries()), "ignore_special_entries"));
    }

    @SuppressWarnings("unchecked")
    private static PathEntry pathEntry(Object value) {
        if (!(value instanceof Map<?, ?> rawEntry)) {
            throw new IllegalArgumentException("entry must be an object");
        }
        Map<String, Object> entry = (Map<String, Object>) rawEntry;
        requireOnly(entry, Set.of("relative_path", "kind"));
        return new PathEntry(
                string(required(entry, "relative_path"), "relative_path"),
                EntryKind.valueOf(string(required(entry, "kind"), "kind")));
    }

    private static Map<String, Object> matchResult(MatchResult result) {
        LinkedHashMap<String, Object> value = new LinkedHashMap<>();
        value.put("ignored", result.ignored());
        value.put("line_number", result.lineNumber());
        value.put("negated", result.negated());
        value.put("pattern", result.pattern());
        value.put("rule_kind", result.ruleKind().name());
        value.put("source_name", result.sourceName());
        return value;
    }

    private static Map<String, Object> pathEntryJson(PathEntry entry) {
        return Map.of("relative_path", entry.relativePath(), "kind", entry.kind().name());
    }

    private static Object required(Map<String, Object> map, String name) {
        if (!map.containsKey(name)) {
            throw new IllegalArgumentException(name + " is required");
        }
        return map.get(name);
    }

    private static void requireOnly(Map<String, Object> map, Set<String> names) {
        for (String key : map.keySet()) {
            if (!names.contains(key)) {
                throw new IllegalArgumentException(key + " is not allowed");
            }
        }
    }

    private static String string(Object value, String name) {
        if (!(value instanceof String string)) {
            throw new IllegalArgumentException(name + " must be a string");
        }
        return string;
    }

    private static boolean bool(Object value, String name) {
        if (!(value instanceof Boolean bool)) {
            throw new IllegalArgumentException(name + " must be a boolean");
        }
        return bool;
    }

    private static Set<String> stringSet(Object value, String name) {
        if (value instanceof Set<?> set) {
            ArrayList<String> copy = new ArrayList<>();
            for (Object item : set) {
                copy.add(string(item, name));
            }
            return Set.copyOf(copy);
        }
        if (!(value instanceof List<?> list)) {
            throw new IllegalArgumentException(name + " must be an array");
        }
        ArrayList<String> copy = new ArrayList<>();
        for (Object item : list) {
            copy.add(string(item, name));
        }
        return Set.copyOf(copy);
    }

    private static Map<String, Object> error(int code, String message) {
        return Map.of("error", Map.of("code", code, "message", message));
    }

    private static String response(Object id, Map<String, Object> body) {
        LinkedHashMap<String, Object> response = new LinkedHashMap<>();
        response.put("jsonrpc", "2.0");
        response.put("id", id);
        response.putAll(body);
        return Json.stringify(response);
    }

    private static void shutdown() {
        running = false;
        try {
            if (serverSocket != null) {
                serverSocket.close();
            }
        } catch (IOException ignored) {
        }
        for (Socket socket : sockets) {
            try {
                socket.close();
            } catch (IOException ignored) {
            }
        }
    }

    private static void shutdownSoon() {
        Thread thread = new Thread(() -> {
            try {
                Thread.sleep(25L);
            } catch (InterruptedException ignored) {
                Thread.currentThread().interrupt();
            }
            shutdown();
        });
        thread.setDaemon(false);
        thread.start();
    }
}
