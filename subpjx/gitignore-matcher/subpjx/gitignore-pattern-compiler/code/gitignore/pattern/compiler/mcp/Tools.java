package gitignore.pattern.compiler.mcp;

import gitignore.pattern.compiler.CompileException;
import gitignore.pattern.compiler.CompileResult;
import gitignore.pattern.compiler.CompiledPattern;
import gitignore.pattern.compiler.Compiler;
import gitignore.pattern.compiler.Diagnostic;
import gitignore.pattern.compiler.PatternSet;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class Tools {

    private Tools() {}

    public static Map<String, Object> list() {
        List<Map<String, Object>> tools = new ArrayList<>();
        tools.add(entry("compile-patterns",
                "Compile gitignore-format text into a PatternSet and Diagnostics.",
                compileInput(), compileOutput()));
        tools.add(entry("compile_patterns",
                "Compile gitignore-format text into a PatternSet and Diagnostics.",
                compileInput(), compileOutput()));
        tools.add(entry("empty-pattern-set",
                "Return an empty PatternSet.",
                emptyInput(), emptyOutput()));
        tools.add(entry("empty_pattern_set",
                "Return an empty PatternSet.",
                emptyInput(), emptyOutput()));
        tools.add(entry("matches",
                "Test whether a path matches a compiled pattern's body.",
                matchesInput(), matchesOutput()));
        tools.add(entry("pattern-at",
                "Return the i-th CompiledPattern in a PatternSet.",
                patternAtInput(), patternAtOutput()));
        tools.add(entry("pattern-count",
                "Return the number of compiled patterns in a PatternSet.",
                patternCountInput(), patternCountOutput()));
        tools.add(entry("pattern_at",
                "Return the i-th CompiledPattern in a PatternSet.",
                patternAtInput(), patternAtOutput()));
        tools.add(entry("pattern_count",
                "Return the number of compiled patterns in a PatternSet.",
                patternCountInput(), patternCountOutput()));

        Map<String, Object> result = new LinkedHashMap<>();
        result.put("tools", tools);
        return result;
    }

    private static Map<String, Object> entry(String name, String description,
                                             Map<String, Object> inputSchema,
                                             Map<String, Object> outputSchema) {
        Map<String, Object> e = new LinkedHashMap<>();
        e.put("name", name);
        e.put("description", description);
        e.put("inputSchema", inputSchema);
        e.put("outputSchema", outputSchema);
        return e;
    }

    private static Map<String, Object> obj() {
        return new LinkedHashMap<>();
    }

    private static Map<String, Object> schema(Map<String, Object> properties, List<String> required) {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("type", "object");
        s.put("properties", properties);
        if (required != null) s.put("required", required);
        s.put("additionalProperties", false);
        return s;
    }

    private static Map<String, Object> typed(String type) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("type", type);
        return m;
    }

    private static Map<String, Object> compileInput() {
        Map<String, Object> p = obj();
        p.put("text", typed("string"));
        return schema(p, List.of("text"));
    }

    private static Map<String, Object> compileOutput() {
        Map<String, Object> p = obj();
        p.put("patternSet", typed("object"));
        p.put("pattern_set", typed("object"));
        p.put("set", typed("object"));
        p.put("diagnostics", typed("array"));
        p.put("content", typed("array"));
        return schema(p, null);
    }

    private static Map<String, Object> emptyInput() {
        return schema(obj(), null);
    }

    private static Map<String, Object> emptyOutput() {
        Map<String, Object> p = obj();
        p.put("patterns", typed("array"));
        p.put("content", typed("array"));
        return schema(p, null);
    }

    private static Map<String, Object> patternCountInput() {
        Map<String, Object> p = obj();
        p.put("set", typed("object"));
        p.put("pattern_set", typed("object"));
        p.put("patternSet", typed("object"));
        return schema(p, null);
    }

    private static Map<String, Object> patternCountOutput() {
        Map<String, Object> p = obj();
        p.put("count", typed("integer"));
        p.put("content", typed("array"));
        return schema(p, null);
    }

    private static Map<String, Object> patternAtInput() {
        Map<String, Object> p = obj();
        p.put("set", typed("object"));
        p.put("pattern_set", typed("object"));
        p.put("patternSet", typed("object"));
        p.put("index", typed("integer"));
        return schema(p, List.of("index"));
    }

    private static Map<String, Object> patternAtOutput() {
        Map<String, Object> p = obj();
        p.put("source", typed("string"));
        p.put("is_negation", typed("boolean"));
        p.put("is_anchored", typed("boolean"));
        p.put("is_dir_only", typed("boolean"));
        p.put("body", typed("string"));
        p.put("regex", typed("string"));
        p.put("pattern", typed("object"));
        p.put("content", typed("array"));
        return schema(p, null);
    }

    private static Map<String, Object> matchesInput() {
        Map<String, Object> p = obj();
        p.put("path", typed("string"));
        p.put("pattern_set", typed("object"));
        p.put("patternSet", typed("object"));
        p.put("set", typed("object"));
        p.put("index", typed("integer"));
        p.put("compiled_pattern", typed("object"));
        p.put("compiledPattern", typed("object"));
        p.put("pattern", typed("object"));
        return schema(p, List.of("path"));
    }

    private static Map<String, Object> matchesOutput() {
        Map<String, Object> p = obj();
        p.put("matches", typed("boolean"));
        p.put("content", typed("array"));
        return schema(p, null);
    }

    public static Map<String, Object> call(String name, Map<String, Object> args) throws ToolException {
        if (name == null) throw new ToolException("invalid argument: name is required");
        switch (name) {
            case "compile-patterns":
            case "compile_patterns":
                return callCompile(args);
            case "empty-pattern-set":
            case "empty_pattern_set":
                return callEmpty(args);
            case "pattern-count":
            case "pattern_count":
                return callCount(args);
            case "pattern-at":
            case "pattern_at":
                return callAt(args);
            case "matches":
                return callMatches(args);
            default:
                throw new ToolException("unknown tool: " + name);
        }
    }

    private static Map<String, Object> callCompile(Map<String, Object> args) throws ToolException {
        Object t = args.get("text");
        if (!(t instanceof String text)) {
            throw new ToolException("invalid argument: text is required");
        }
        CompileResult cr = Compiler.compilePatterns(text);
        Map<String, Object> set = serializeSet(cr.patternSet());
        List<Map<String, Object>> diags = new ArrayList<>();
        for (Diagnostic d : cr.diagnostics()) {
            Map<String, Object> dm = new LinkedHashMap<>();
            dm.put("line_number", d.lineNumber());
            dm.put("lineNumber", d.lineNumber());
            dm.put("line_text", d.lineText());
            dm.put("lineText", d.lineText());
            dm.put("reason", d.reason());
            diags.add(dm);
        }

        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("patternSet", set);
        payload.put("pattern_set", set);
        payload.put("set", set);
        payload.put("diagnostics", diags);
        return wrapWithContent(payload);
    }

    private static Map<String, Object> callEmpty(Map<String, Object> args) {
        Map<String, Object> set = serializeSet(PatternSet.empty());
        return wrapWithContent(set);
    }

    private static Map<String, Object> callCount(Map<String, Object> args) {
        Object setObj = resolveSet(args);
        int count = 0;
        if (setObj instanceof Map<?, ?> sm) {
            Object pats = sm.get("patterns");
            if (pats instanceof List<?> list) {
                count = list.size();
            }
        }
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("count", count);
        return wrapWithContent(payload);
    }

    private static Map<String, Object> callAt(Map<String, Object> args) throws ToolException {
        Object setObj = resolveSet(args);
        Object idxObj = args.get("index");
        if (!(idxObj instanceof Number n)) {
            throw new ToolException("invalid argument: index is required");
        }
        int index = n.intValue();
        if (!(setObj instanceof Map<?, ?> sm)) {
            throw new ToolException("invalid argument: set is required");
        }
        Object pats = sm.get("patterns");
        if (!(pats instanceof List<?> list)) {
            throw new ToolException("invalid argument: malformed set");
        }
        if (index < 0 || index >= list.size()) {
            throw new ToolException("invalid argument: index out of range");
        }
        Object raw = list.get(index);
        if (!(raw instanceof Map<?, ?> rm)) {
            throw new ToolException("invalid argument: malformed pattern");
        }
        Map<String, Object> pat = new LinkedHashMap<>();
        for (Map.Entry<?, ?> e : rm.entrySet()) {
            pat.put(e.getKey().toString(), e.getValue());
        }
        Map<String, Object> payload = new LinkedHashMap<>(pat);
        payload.put("pattern", new LinkedHashMap<>(pat));
        return wrapWithContent(payload);
    }

    private static Map<String, Object> callMatches(Map<String, Object> args) throws ToolException {
        Object pathObj = args.get("path");
        if (!(pathObj instanceof String path)) {
            throw new ToolException("invalid argument: path is required");
        }
        String body = null;

        Object setObj = resolveSet(args);
        Object idxObj = args.get("index");
        if (setObj instanceof Map<?, ?> sm && idxObj instanceof Number n) {
            Object pats = sm.get("patterns");
            if (pats instanceof List<?> list) {
                int idx = n.intValue();
                if (idx >= 0 && idx < list.size()) {
                    Object pat = list.get(idx);
                    body = extractBody(pat);
                }
            }
        }

        if (body == null) {
            Object cp = args.get("compiled_pattern");
            if (cp == null) cp = args.get("compiledPattern");
            if (cp == null) cp = args.get("pattern");
            body = extractBody(cp);
        }

        if (body == null) {
            throw new ToolException("invalid argument: no pattern provided");
        }

        boolean m;
        try {
            String regex = Compiler.compileBody(body);
            m = java.util.regex.Pattern.compile(regex).matcher(path).matches();
        } catch (CompileException e) {
            throw new ToolException("compile error: " + e.getMessage());
        }

        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("matches", m);
        return wrapWithContent(payload);
    }

    private static String extractBody(Object o) {
        if (!(o instanceof Map<?, ?> m)) return null;
        Object b = m.get("body");
        if (b instanceof String s) return s;
        Object inner = m.get("pattern");
        if (inner instanceof Map<?, ?> im) {
            Object b2 = im.get("body");
            if (b2 instanceof String s) return s;
        }
        return null;
    }

    private static Object resolveSet(Map<String, Object> args) {
        Object o = args.get("set");
        if (o == null) o = args.get("pattern_set");
        if (o == null) o = args.get("patternSet");
        return o;
    }

    private static Map<String, Object> serializeSet(PatternSet ps) {
        Map<String, Object> m = new LinkedHashMap<>();
        List<Map<String, Object>> pats = new ArrayList<>();
        for (CompiledPattern cp : ps.patterns()) {
            pats.add(serializePattern(cp));
        }
        m.put("patterns", pats);
        return m;
    }

    private static Map<String, Object> serializePattern(CompiledPattern cp) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("source", cp.source());
        m.put("is_negation", cp.isNegation());
        m.put("isNegation", cp.isNegation());
        m.put("is_anchored", cp.isAnchored());
        m.put("isAnchored", cp.isAnchored());
        m.put("is_dir_only", cp.isDirOnly());
        m.put("isDirOnly", cp.isDirOnly());
        m.put("body", cp.body());
        m.put("regex", cp.regex());
        return m;
    }

    private static Map<String, Object> wrapWithContent(Map<String, Object> payload) {
        String text = Json.emit(payload);
        Map<String, Object> result = new LinkedHashMap<>(payload);
        Map<String, Object> textBlock = new LinkedHashMap<>();
        textBlock.put("type", "text");
        textBlock.put("text", text);
        List<Map<String, Object>> content = new ArrayList<>();
        content.add(textBlock);
        result.put("content", content);
        return result;
    }
}
