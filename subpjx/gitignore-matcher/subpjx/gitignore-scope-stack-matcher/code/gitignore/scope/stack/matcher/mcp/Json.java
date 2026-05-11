package gitignore.scope.stack.matcher.mcp;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/** Minimal JSON parser and writer for the MCP wire protocol. */
final class Json {
    private Json() {}

    public static Object parse(String s) {
        Parser p = new Parser(s);
        p.skipWs();
        Object v = p.readValue();
        p.skipWs();
        return v;
    }

    /** Write `v` as JSON with object keys sorted lexicographically for stable bytes. */
    public static String write(Object v) {
        StringBuilder sb = new StringBuilder();
        writeValue(sb, v);
        return sb.toString();
    }

    private static void writeValue(StringBuilder sb, Object v) {
        if (v == null) { sb.append("null"); return; }
        if (v instanceof Boolean b) { sb.append(b ? "true" : "false"); return; }
        if (v instanceof Number n) { writeNumber(sb, n); return; }
        if (v instanceof String s) { writeString(sb, s); return; }
        if (v instanceof Map<?, ?> m) { writeObject(sb, m); return; }
        if (v instanceof List<?> l) { writeArray(sb, l); return; }
        if (v instanceof Object[] a) { writeArray(sb, List.of(a)); return; }
        throw new IllegalArgumentException("cannot serialize: " + v.getClass());
    }

    private static void writeNumber(StringBuilder sb, Number n) {
        if (n instanceof Double || n instanceof Float) {
            double d = n.doubleValue();
            if (d == Math.floor(d) && !Double.isInfinite(d)) {
                sb.append(Long.toString((long) d));
            } else {
                sb.append(Double.toString(d));
            }
        } else {
            sb.append(n.toString());
        }
    }

    private static void writeString(StringBuilder sb, String s) {
        sb.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"':  sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\b': sb.append("\\b"); break;
                case '\f': sb.append("\\f"); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
                default:
                    if (c < 0x20) sb.append(String.format("\\u%04x", (int) c));
                    else sb.append(c);
            }
        }
        sb.append('"');
    }

    @SuppressWarnings("unchecked")
    private static void writeObject(StringBuilder sb, Map<?, ?> m) {
        Map<String, Object> sorted = new TreeMap<>();
        for (Map.Entry<?, ?> e : m.entrySet()) {
            sorted.put((String) e.getKey(), e.getValue());
        }
        sb.append('{');
        boolean first = true;
        for (Map.Entry<String, Object> e : sorted.entrySet()) {
            if (!first) sb.append(',');
            first = false;
            writeString(sb, e.getKey());
            sb.append(':');
            writeValue(sb, e.getValue());
        }
        sb.append('}');
    }

    private static void writeArray(StringBuilder sb, List<?> a) {
        sb.append('[');
        boolean first = true;
        for (Object item : a) {
            if (!first) sb.append(',');
            first = false;
            writeValue(sb, item);
        }
        sb.append(']');
    }

    private static final class Parser {
        private final String s;
        private int i;

        Parser(String s) { this.s = s; this.i = 0; }

        void skipWs() {
            while (i < s.length()) {
                char c = s.charAt(i);
                if (c == ' ' || c == '\t' || c == '\n' || c == '\r') i++;
                else break;
            }
        }

        Object readValue() {
            skipWs();
            if (i >= s.length()) throw new IllegalArgumentException("unexpected end of input");
            char c = s.charAt(i);
            if (c == '{') return readObject();
            if (c == '[') return readArray();
            if (c == '"') return readString();
            if (c == 't' || c == 'f') return readBool();
            if (c == 'n') return readNull();
            return readNumber();
        }

        Map<String, Object> readObject() {
            i++; // consume '{'
            Map<String, Object> out = new LinkedHashMap<>();
            skipWs();
            if (i < s.length() && s.charAt(i) == '}') { i++; return out; }
            while (true) {
                skipWs();
                String key = readString();
                skipWs();
                if (i >= s.length() || s.charAt(i) != ':') {
                    throw new IllegalArgumentException("expected ':' at " + i);
                }
                i++; // consume ':'
                Object value = readValue();
                out.put(key, value);
                skipWs();
                if (i < s.length() && s.charAt(i) == ',') { i++; continue; }
                if (i < s.length() && s.charAt(i) == '}') { i++; return out; }
                throw new IllegalArgumentException("expected ',' or '}' at " + i);
            }
        }

        List<Object> readArray() {
            i++; // consume '['
            List<Object> out = new ArrayList<>();
            skipWs();
            if (i < s.length() && s.charAt(i) == ']') { i++; return out; }
            while (true) {
                Object value = readValue();
                out.add(value);
                skipWs();
                if (i < s.length() && s.charAt(i) == ',') { i++; continue; }
                if (i < s.length() && s.charAt(i) == ']') { i++; return out; }
                throw new IllegalArgumentException("expected ',' or ']' at " + i);
            }
        }

        String readString() {
            if (s.charAt(i) != '"') throw new IllegalArgumentException("expected string at " + i);
            i++;
            StringBuilder sb = new StringBuilder();
            while (i < s.length()) {
                char c = s.charAt(i++);
                if (c == '"') return sb.toString();
                if (c == '\\') {
                    if (i >= s.length()) throw new IllegalArgumentException("bad escape");
                    char e = s.charAt(i++);
                    switch (e) {
                        case '"':  sb.append('"'); break;
                        case '\\': sb.append('\\'); break;
                        case '/':  sb.append('/'); break;
                        case 'b':  sb.append('\b'); break;
                        case 'f':  sb.append('\f'); break;
                        case 'n':  sb.append('\n'); break;
                        case 'r':  sb.append('\r'); break;
                        case 't':  sb.append('\t'); break;
                        case 'u':
                            if (i + 4 > s.length()) throw new IllegalArgumentException("bad unicode escape");
                            sb.append((char) Integer.parseInt(s.substring(i, i + 4), 16));
                            i += 4;
                            break;
                        default: throw new IllegalArgumentException("bad escape: " + e);
                    }
                } else {
                    sb.append(c);
                }
            }
            throw new IllegalArgumentException("unterminated string");
        }

        Boolean readBool() {
            if (s.startsWith("true", i))  { i += 4; return Boolean.TRUE; }
            if (s.startsWith("false", i)) { i += 5; return Boolean.FALSE; }
            throw new IllegalArgumentException("bad literal at " + i);
        }

        Object readNull() {
            if (s.startsWith("null", i)) { i += 4; return null; }
            throw new IllegalArgumentException("bad literal at " + i);
        }

        Number readNumber() {
            int start = i;
            if (i < s.length() && s.charAt(i) == '-') i++;
            while (i < s.length() && Character.isDigit(s.charAt(i))) i++;
            boolean isFloat = false;
            if (i < s.length() && s.charAt(i) == '.') {
                isFloat = true;
                i++;
                while (i < s.length() && Character.isDigit(s.charAt(i))) i++;
            }
            if (i < s.length() && (s.charAt(i) == 'e' || s.charAt(i) == 'E')) {
                isFloat = true;
                i++;
                if (i < s.length() && (s.charAt(i) == '+' || s.charAt(i) == '-')) i++;
                while (i < s.length() && Character.isDigit(s.charAt(i))) i++;
            }
            String num = s.substring(start, i);
            if (isFloat) return Double.parseDouble(num);
            return Long.parseLong(num);
        }
    }
}
