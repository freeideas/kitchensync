package gitignore.matcher.mcp;

import java.util.List;
import java.util.Map;

final class Schemas {
    private Schemas() {}

    static Map<String, Object> toolsList() {
        return Map.of("tools", List.of(
                compileTool(),
                emptyTool(),
                extendTool(),
                filterTool(),
                matchTool()));
    }

    private static Map<String, Object> compileTool() {
        return Map.of(
                "name", "compile",
                "description", "Compile ordered gitignore pattern layers into an immutable matcher.",
                "inputSchema", Map.of(
                        "type", "object",
                        "properties", Map.of("layers", layersSchema(), "options", optionsSchema()),
                        "required", List.of("layers"),
                        "additionalProperties", false),
                "outputSchema", matcherIdSchema());
    }

    private static Map<String, Object> emptyTool() {
        return Map.of(
                "name", "empty",
                "description", "Create an immutable matcher with only built-in exclusions.",
                "inputSchema", Map.of(
                        "type", "object",
                        "properties", Map.of("options", optionsSchema()),
                        "required", List.of(),
                        "additionalProperties", false),
                "outputSchema", matcherIdSchema());
    }

    private static Map<String, Object> extendTool() {
        return Map.of(
                "name", "extend",
                "description", "Return a new matcher with one pattern layer appended.",
                "inputSchema", Map.of(
                        "type", "object",
                        "properties", Map.of("matcher", Map.of("type", "string"), "layer", layerSchema()),
                        "required", List.of("matcher", "layer"),
                        "additionalProperties", false),
                "outputSchema", matcherIdSchema());
    }

    private static Map<String, Object> matchTool() {
        return Map.of(
                "name", "match",
                "description", "Match one path entry against ordered gitignore pattern layers.",
                "inputSchema", Map.of(
                        "type", "object",
                        "properties", Map.of("matcher", Map.of("type", "string"), "entry", pathEntrySchema()),
                        "required", List.of("matcher", "entry"),
                        "additionalProperties", false),
                "outputSchema", matchResultSchema());
    }

    private static Map<String, Object> filterTool() {
        return Map.of(
                "name", "filter",
                "description", "Return the path entries that are not ignored.",
                "inputSchema", Map.of(
                        "type", "object",
                        "properties", Map.of(
                                "matcher", Map.of("type", "string"),
                                "entries", Map.of("type", "array", "items", pathEntrySchema())),
                        "required", List.of("matcher", "entries"),
                        "additionalProperties", false),
                "outputSchema", Map.of(
                        "type", "object",
                        "properties", Map.of("entries", Map.of("type", "array", "items", pathEntrySchema())),
                        "required", List.of("entries"),
                        "additionalProperties", false));
    }

    private static Map<String, Object> matcherIdSchema() {
        return Map.of(
                "type", "object",
                "properties", Map.of("matcher_id", Map.of("type", "string")),
                "required", List.of("matcher_id"),
                "additionalProperties", false);
    }

    private static Map<String, Object> layersSchema() {
        return Map.of("type", "array", "items", layerSchema());
    }

    private static Map<String, Object> layerSchema() {
        return Map.of(
                "type", "object",
                "properties", Map.of(
                        "base_path", Map.of("type", "string"),
                        "pattern_text", Map.of("type", "string"),
                        "source_name", Map.of("type", List.of("string", "null"))),
                "required", List.of("base_path", "pattern_text"),
                "additionalProperties", false);
    }

    private static Map<String, Object> optionsSchema() {
        return Map.of(
                "type", "object",
                "properties", Map.of(
                        "always_excluded_directory_names", Map.of("type", "array", "items", Map.of("type", "string")),
                        "default_excluded_directory_names", Map.of("type", "array", "items", Map.of("type", "string")),
                        "ignore_symlinks", Map.of("type", "boolean"),
                        "ignore_special_entries", Map.of("type", "boolean")),
                "required", List.of(),
                "additionalProperties", false);
    }

    private static Map<String, Object> pathEntrySchema() {
        return Map.of(
                "type", "object",
                "properties", Map.of(
                        "relative_path", Map.of("type", "string"),
                        "kind", Map.of("type", "string", "enum", List.of("regular_file", "directory", "symlink", "special"))),
                "required", List.of("relative_path", "kind"),
                "additionalProperties", false);
    }

    private static Map<String, Object> matchResultSchema() {
        return Map.of(
                "type", "object",
                "properties", Map.of(
                        "ignored", Map.of("type", "boolean"),
                        "rule_kind", Map.of("type", "string", "enum", List.of("always_builtin", "default_builtin", "pattern", "none")),
                        "negated", Map.of("type", "boolean"),
                        "source_name", Map.of("type", List.of("string", "null")),
                        "line_number", Map.of("type", List.of("integer", "null")),
                        "pattern", Map.of("type", List.of("string", "null"))),
                "required", List.of("ignored", "rule_kind", "negated", "source_name", "line_number", "pattern"),
                "additionalProperties", false);
    }
}
