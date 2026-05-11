package gitignore.scope.stack.matcher.mcp;

import gitignore.scope.stack.matcher.CompiledPattern;
import gitignore.scope.stack.matcher.EntryKind;
import gitignore.scope.stack.matcher.Eval;
import gitignore.scope.stack.matcher.Layer;
import gitignore.scope.stack.matcher.Matcher;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/** Registry and dispatch for MCP tools. */
final class Tools {
    private Tools() {}

    /** Returns the list of tools sorted alphabetically by name with stable schemas. */
    static List<Map<String, Object>> list() {
        Map<String, Map<String, Object>> sorted = new TreeMap<>();
        for (Map<String, Object> tool : DEFINITIONS) {
            sorted.put((String) tool.get("name"), tool);
        }
        return new ArrayList<>(sorted.values());
    }

    static Map<String, Object> call(String name, Map<String, Object> args) {
        switch (name) {
            case "empty-matcher":
            case "empty_matcher":
                return callEmptyMatcher();
            case "push-scope":
                return callPushScope(args);
            case "layer-count":
                return callLayerCount(args);
            case "layer-at":
                return callLayerAt(args);
            case "is-ignored":
            case "is_ignored":
                return callIsIgnored(args);
            case "is-ignored-entry":
            case "is_ignored_entry":
                return callIsIgnoredEntry(args);
            default:
                throw new RuntimeException("invalid argument: unknown tool: " + name);
        }
    }

    private static Map<String, Object> callEmptyMatcher() {
        Map<String, Object> matcher = matcherToJson(Matcher.empty());
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("matcher", matcher);
        attachContent(result, matcher);
        return result;
    }

    private static Map<String, Object> callPushScope(Map<String, Object> args) {
        Matcher parent = matcherFromJson(args.get("matcher"));
        String scopeDir = requireString(args, "scope_dir");
        List<CompiledPattern> patternSet = patternSetFromJson(args.get("pattern_set"));
        Matcher next = Matcher.pushScope(parent, scopeDir, patternSet);
        Map<String, Object> matcher = matcherToJson(next);
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("matcher", matcher);
        attachContent(result, matcher);
        return result;
    }

    private static Map<String, Object> callLayerCount(Map<String, Object> args) {
        Matcher m = matcherFromJson(args.get("matcher"));
        long count = m.layerCount();
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("count", count);
        attachContent(result, count);
        return result;
    }

    private static Map<String, Object> callLayerAt(Map<String, Object> args) {
        Matcher m = matcherFromJson(args.get("matcher"));
        Object idxObj = args.get("index");
        int idx;
        if (idxObj instanceof Number n) idx = n.intValue();
        else throw new RuntimeException("invalid argument: index must be an integer");
        Layer layer = m.layerAt(idx);
        Map<String, Object> layerJson = layerToJson(layer);
        Map<String, Object> result = new LinkedHashMap<>(layerJson);
        attachContent(result, layerJson);
        return result;
    }

    private static Map<String, Object> callIsIgnored(Map<String, Object> args) {
        Matcher m = matcherFromJson(args.get("matcher"));
        String path = requireString(args, "path");
        Object dirObj = args.containsKey("is_dir") ? args.get("is_dir") : args.get("isDir");
        if (!(dirObj instanceof Boolean isDir)) {
            throw new RuntimeException("invalid argument: is_dir must be boolean");
        }
        boolean ignored = Eval.isIgnored(m, path, isDir);
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("ignored", ignored);
        attachContent(result, ignored);
        return result;
    }

    private static Map<String, Object> callIsIgnoredEntry(Map<String, Object> args) {
        Matcher m = matcherFromJson(args.get("matcher"));
        String path = requireString(args, "path");
        String kindStr = requireString(args, "kind");
        boolean ignored = Eval.isIgnoredEntry(m, path, EntryKind.fromString(kindStr));
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("ignored", ignored);
        attachContent(result, ignored);
        return result;
    }

    private static void attachContent(Map<String, Object> result, Object payload) {
        Map<String, Object> textItem = new LinkedHashMap<>();
        textItem.put("type", "text");
        textItem.put("text", Json.write(payload));
        result.put("content", List.of(textItem));
    }

    // ---- Matcher <-> JSON ---------------------------------------------------

    private static Map<String, Object> matcherToJson(Matcher m) {
        List<Object> layers = new ArrayList<>();
        for (Layer layer : m.layers()) {
            layers.add(layerToJson(layer));
        }
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("layers", layers);
        return out;
    }

    private static Map<String, Object> layerToJson(Layer layer) {
        List<Object> patterns = new ArrayList<>();
        for (CompiledPattern p : layer.patternSet()) {
            patterns.add(patternToJson(p));
        }
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("scope_dir", layer.scopeDir());
        out.put("pattern_set", patterns);
        return out;
    }

    private static Map<String, Object> patternToJson(CompiledPattern p) {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("body", p.body());
        out.put("is_negation", p.isNegation());
        out.put("is_anchored", p.isAnchored());
        out.put("is_dir_only", p.isDirOnly());
        return out;
    }

    @SuppressWarnings("unchecked")
    private static Matcher matcherFromJson(Object obj) {
        if (!(obj instanceof Map<?, ?> map)) {
            throw new RuntimeException("invalid argument: matcher must be an object");
        }
        Object layersObj = map.get("layers");
        if (!(layersObj instanceof List<?> layersList)) {
            throw new RuntimeException("invalid argument: matcher.layers must be an array");
        }
        Matcher m = Matcher.empty();
        for (Object layerObj : layersList) {
            if (!(layerObj instanceof Map<?, ?> layerMap)) {
                throw new RuntimeException("invalid argument: layer must be an object");
            }
            String scopeDir = (String) layerMap.get("scope_dir");
            if (scopeDir == null) scopeDir = "";
            List<CompiledPattern> ps = patternSetFromJson(layerMap.get("pattern_set"));
            m = Matcher.pushScope(m, scopeDir, ps);
        }
        return m;
    }

    @SuppressWarnings("unchecked")
    private static List<CompiledPattern> patternSetFromJson(Object obj) {
        List<CompiledPattern> out = new ArrayList<>();
        if (obj == null) return out;
        if (!(obj instanceof List<?> list)) {
            throw new RuntimeException("invalid argument: pattern_set must be an array");
        }
        for (Object item : list) {
            if (!(item instanceof Map<?, ?> p)) {
                throw new RuntimeException("invalid argument: pattern must be an object");
            }
            String body = (String) p.get("body");
            if (body == null) throw new RuntimeException("invalid argument: pattern.body required");
            boolean isNegation = Boolean.TRUE.equals(p.get("is_negation"));
            boolean isAnchored = Boolean.TRUE.equals(p.get("is_anchored"));
            boolean isDirOnly  = Boolean.TRUE.equals(p.get("is_dir_only"));
            out.add(new CompiledPattern(body, isNegation, isAnchored, isDirOnly));
        }
        return out;
    }

    private static String requireString(Map<String, Object> args, String key) {
        Object v = args.get(key);
        if (v instanceof String s) return s;
        throw new RuntimeException("invalid argument: " + key + " must be a string");
    }

    // ---- Tool definitions (schemas for tools/list) --------------------------

    private static final List<Map<String, Object>> DEFINITIONS = buildDefinitions();

    private static List<Map<String, Object>> buildDefinitions() {
        List<Map<String, Object>> defs = new ArrayList<>();

        defs.add(tool(
            "empty-matcher",
            "Return a Matcher with no user layers; only built-in excludes apply.",
            emptyObjectSchema(),
            matcherResultSchema()
        ));
        defs.add(tool(
            "empty_matcher",
            "Return a Matcher with no user layers; only built-in excludes apply.",
            emptyObjectSchema(),
            matcherResultSchema()
        ));

        defs.add(tool(
            "push-scope",
            "Append a new layer with a scope directory and pattern set to a matcher.",
            pushScopeInputSchema(),
            matcherResultSchema()
        ));

        defs.add(tool(
            "layer-count",
            "Return the number of scope layers in a matcher.",
            matcherOnlyInputSchema(),
            object(
                propsOf("count", typed("integer")),
                List.of("count")
            )
        ));

        defs.add(tool(
            "layer-at",
            "Return the layer at the given zero-based index of a matcher.",
            layerAtInputSchema(),
            object(
                Map.of(
                    "scope_dir", typed("string"),
                    "pattern_set", patternSetSchema()
                ),
                List.of("scope_dir", "pattern_set")
            )
        ));

        defs.add(tool(
            "is-ignored",
            "Decide whether a path is ignored under the matcher.",
            isIgnoredInputSchema(),
            ignoredResultSchema()
        ));
        defs.add(tool(
            "is_ignored",
            "Decide whether a path is ignored under the matcher.",
            isIgnoredInputSchema(),
            ignoredResultSchema()
        ));

        defs.add(tool(
            "is-ignored-entry",
            "Decide whether a filesystem entry of a given kind is ignored under the matcher.",
            isIgnoredEntryInputSchema(),
            ignoredResultSchema()
        ));
        defs.add(tool(
            "is_ignored_entry",
            "Decide whether a filesystem entry of a given kind is ignored under the matcher.",
            isIgnoredEntryInputSchema(),
            ignoredResultSchema()
        ));

        return defs;
    }

    private static Map<String, Object> tool(String name, String description,
                                            Map<String, Object> input, Map<String, Object> output) {
        Map<String, Object> t = new LinkedHashMap<>();
        t.put("name", name);
        t.put("description", description);
        t.put("inputSchema", input);
        t.put("outputSchema", output);
        return t;
    }

    private static Map<String, Object> typed(String type) {
        Map<String, Object> o = new LinkedHashMap<>();
        o.put("type", type);
        return o;
    }

    private static Map<String, Object> object(Map<String, Object> props, List<String> required) {
        Map<String, Object> o = new LinkedHashMap<>();
        o.put("type", "object");
        o.put("properties", props);
        o.put("required", required);
        o.put("additionalProperties", false);
        return o;
    }

    private static Map<String, Object> propsOf(String k1, Object v1) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put(k1, v1);
        return m;
    }

    private static Map<String, Object> patternSchema() {
        Map<String, Object> props = new LinkedHashMap<>();
        props.put("body", typed("string"));
        props.put("is_negation", typed("boolean"));
        props.put("is_anchored", typed("boolean"));
        props.put("is_dir_only", typed("boolean"));
        return object(props, List.of("body", "is_negation", "is_anchored", "is_dir_only"));
    }

    private static Map<String, Object> patternSetSchema() {
        Map<String, Object> o = new LinkedHashMap<>();
        o.put("type", "array");
        o.put("items", patternSchema());
        return o;
    }

    private static Map<String, Object> layerSchema() {
        Map<String, Object> props = new LinkedHashMap<>();
        props.put("scope_dir", typed("string"));
        props.put("pattern_set", patternSetSchema());
        return object(props, List.of("scope_dir", "pattern_set"));
    }

    private static Map<String, Object> matcherSchema() {
        Map<String, Object> layers = new LinkedHashMap<>();
        layers.put("type", "array");
        layers.put("items", layerSchema());
        return object(propsOf("layers", layers), List.of("layers"));
    }

    private static Map<String, Object> emptyObjectSchema() {
        Map<String, Object> o = new LinkedHashMap<>();
        o.put("type", "object");
        o.put("properties", new LinkedHashMap<String, Object>());
        o.put("additionalProperties", false);
        return o;
    }

    private static Map<String, Object> matcherResultSchema() {
        return object(propsOf("matcher", matcherSchema()), List.of("matcher"));
    }

    private static Map<String, Object> matcherOnlyInputSchema() {
        return object(propsOf("matcher", matcherSchema()), List.of("matcher"));
    }

    private static Map<String, Object> pushScopeInputSchema() {
        Map<String, Object> props = new LinkedHashMap<>();
        props.put("matcher", matcherSchema());
        props.put("scope_dir", typed("string"));
        props.put("pattern_set", patternSetSchema());
        return object(props, List.of("matcher", "scope_dir", "pattern_set"));
    }

    private static Map<String, Object> layerAtInputSchema() {
        Map<String, Object> props = new LinkedHashMap<>();
        props.put("matcher", matcherSchema());
        Map<String, Object> indexSchema = new LinkedHashMap<>();
        indexSchema.put("type", "integer");
        indexSchema.put("minimum", 0L);
        props.put("index", indexSchema);
        return object(props, List.of("matcher", "index"));
    }

    private static Map<String, Object> isIgnoredInputSchema() {
        Map<String, Object> props = new LinkedHashMap<>();
        props.put("matcher", matcherSchema());
        props.put("path", typed("string"));
        props.put("is_dir", typed("boolean"));
        return object(props, List.of("matcher", "path", "is_dir"));
    }

    private static Map<String, Object> isIgnoredEntryInputSchema() {
        Map<String, Object> props = new LinkedHashMap<>();
        props.put("matcher", matcherSchema());
        props.put("path", typed("string"));
        Map<String, Object> kindSchema = new LinkedHashMap<>();
        kindSchema.put("type", "string");
        kindSchema.put("enum", List.of("file", "dir", "symlink", "special"));
        props.put("kind", kindSchema);
        return object(props, List.of("matcher", "path", "kind"));
    }

    private static Map<String, Object> ignoredResultSchema() {
        return object(propsOf("ignored", typed("boolean")), List.of("ignored"));
    }
}
