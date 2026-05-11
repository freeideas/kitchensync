package connection.pool.mcp;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

final class Schemas {

    static Map<String, Object> toolsList() {
        List<Map<String, Object>> tools = new ArrayList<>();
        tools.add(tool(
                "acquire",
                "Acquire a connection from a registered pool, opening or reusing as needed.",
                obj(
                        prop("pool", strType()),
                        required("pool")
                ),
                obj(
                        prop("connection", strType()),
                        prop("open_count", intType()),
                        required("connection")
                )
        ));
        tools.add(tool(
                "close-pool",
                "Shut down a pool, closing every idle connection.",
                obj(prop("pool", strType()), required("pool")),
                obj()
        ));
        tools.add(tool(
                "get-close-count",
                "Return the number of times close has been invoked on connections of this pool.",
                obj(prop("pool", strType()), required("pool")),
                obj(prop("count", intType()), required("count"))
        ));
        tools.add(tool(
                "get-events",
                "Return the recorded acquire/release events for this pool.",
                obj(prop("pool", strType()), required("pool")),
                obj(prop("events", arrayOf(any())), required("events"))
        ));
        tools.add(tool(
                "get-open-count",
                "Return the number of times open has successfully produced a connection for this pool.",
                obj(prop("pool", strType()), required("pool")),
                obj(prop("count", intType()), required("count"))
        ));
        tools.add(tool(
                "register-pool",
                "Register or look up a pool by key; returns a handle to the pool.",
                obj(
                        prop("key", any()),
                        prop("mc", intType()),
                        prop("ct", intType()),
                        prop("ka", intType()),
                        prop("on_event", boolType()),
                        prop("open_delay_ms", intType()),
                        prop("open_fail_count", intType()),
                        required("key")
                ),
                obj(prop("pool", strType()), required("pool"))
        ));
        tools.add(tool(
                "release",
                "Return a connection to the pool's idle set.",
                obj(
                        prop("pool", strType()),
                        prop("connection", strType()),
                        required("pool", "connection")
                ),
                obj(prop("close_count", intType()))
        ));
        tools.sort((a, b) -> ((String) a.get("name")).compareTo((String) b.get("name")));
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("tools", tools);
        return result;
    }

    private static Map<String, Object> tool(String name, String desc,
                                            Map<String, Object> input,
                                            Map<String, Object> output) {
        Map<String, Object> t = new LinkedHashMap<>();
        t.put("name", name);
        t.put("description", desc);
        t.put("inputSchema", input);
        t.put("outputSchema", output);
        return t;
    }

    @SafeVarargs
    private static Map<String, Object> obj(Object... parts) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("type", "object");
        Map<String, Object> props = new LinkedHashMap<>();
        List<String> reqd = new ArrayList<>();
        for (Object part : parts) {
            if (part instanceof Property p) {
                props.put(p.name, p.schema);
            } else if (part instanceof Required r) {
                for (String n : r.names) reqd.add(n);
            }
        }
        m.put("properties", props);
        if (!reqd.isEmpty()) m.put("required", reqd);
        m.put("additionalProperties", Boolean.TRUE);
        return m;
    }

    private static final class Property {
        final String name;
        final Map<String, Object> schema;
        Property(String name, Map<String, Object> schema) {
            this.name = name;
            this.schema = schema;
        }
    }

    private static final class Required {
        final String[] names;
        Required(String... names) { this.names = names; }
    }

    private static Property prop(String name, Map<String, Object> schema) {
        return new Property(name, schema);
    }

    private static Required required(String... names) {
        return new Required(names);
    }

    private static Map<String, Object> strType() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("type", "string");
        return m;
    }

    private static Map<String, Object> intType() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("type", "integer");
        return m;
    }

    private static Map<String, Object> boolType() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("type", "boolean");
        return m;
    }

    private static Map<String, Object> any() {
        return new LinkedHashMap<>();
    }

    private static Map<String, Object> arrayOf(Map<String, Object> items) {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("type", "array");
        m.put("items", items);
        return m;
    }
}
