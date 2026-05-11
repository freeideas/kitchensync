package connection.pool.mcp;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

final class Json {

    static Object parse(String s) {
        Parser p = new Parser(s);
        p.skipWs();
        Object v = p.readValue();
        p.skipWs();
        if (p.pos < p.src.length()) {
            throw new RuntimeException("trailing junk at " + p.pos);
        }
        return v;
    }

    static String stringify(Object v) {
        StringBuilder sb = new StringBuilder();
        write(sb, v, false);
        return sb.toString();
    }

    static String canonical(Object v) {
        StringBuilder sb = new StringBuilder();
        write(sb, v, true);
        return sb.toString();
    }

    private static void write(StringBuilder sb, Object v, boolean sortKeys) {
        if (v == null) {
            sb.append("null");
            return;
        }
        if (v instanceof String s) {
            writeString(sb, s);
            return;
        }
        if (v instanceof Boolean b) {
            sb.append(b.booleanValue() ? "true" : "false");
            return;
        }
        if (v instanceof Number n) {
            if (n instanceof Double d) {
                if (d.isInfinite() || d.isNaN()) {
                    sb.append("null");
                } else if (d == Math.floor(d) && !d.isInfinite()
                        && d >= Long.MIN_VALUE && d <= Long.MAX_VALUE) {
                    sb.append(Long.toString(d.longValue()));
                } else {
                    sb.append(d.toString());
                }
            } else if (n instanceof Float f) {
                sb.append(Float.toString(f));
            } else {
                sb.append(n.toString());
            }
            return;
        }
        if (v instanceof Map<?, ?> m) {
            sb.append('{');
            Iterable<? extends Map.Entry<?, ?>> entries;
            if (sortKeys) {
                TreeMap<String, Object> sorted = new TreeMap<>();
                for (Map.Entry<?, ?> e : m.entrySet()) {
                    sorted.put(String.valueOf(e.getKey()), e.getValue());
                }
                entries = sorted.entrySet();
            } else {
                entries = m.entrySet();
            }
            boolean first = true;
            for (Map.Entry<?, ?> e : entries) {
                if (!first) sb.append(',');
                writeString(sb, String.valueOf(e.getKey()));
                sb.append(':');
                write(sb, e.getValue(), sortKeys);
                first = false;
            }
            sb.append('}');
            return;
        }
        if (v instanceof Iterable<?> it) {
            sb.append('[');
            boolean first = true;
            for (Object x : it) {
                if (!first) sb.append(',');
                write(sb, x, sortKeys);
                first = false;
            }
            sb.append(']');
            return;
        }
        if (v.getClass().isArray()) {
            sb.append('[');
            int n = java.lang.reflect.Array.getLength(v);
            for (int i = 0; i < n; i++) {
                if (i > 0) sb.append(',');
                write(sb, java.lang.reflect.Array.get(v, i), sortKeys);
            }
            sb.append(']');
            return;
        }
        writeString(sb, v.toString());
    }

    private static void writeString(StringBuilder sb, String s) {
        sb.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"': sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
                case '\b': sb.append("\\b"); break;
                case '\f': sb.append("\\f"); break;
                default:
                    if (c < 0x20) {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
            }
        }
        sb.append('"');
    }

    private static final class Parser {
        final String src;
        int pos = 0;

        Parser(String s) { this.src = s; }

        void skipWs() {
            while (pos < src.length()) {
                char c = src.charAt(pos);
                if (c == ' ' || c == '\t' || c == '\n' || c == '\r') pos++;
                else break;
            }
        }

        Object readValue() {
            skipWs();
            if (pos >= src.length()) throw new RuntimeException("unexpected end of input");
            char c = src.charAt(pos);
            if (c == '{') return readObject();
            if (c == '[') return readArray();
            if (c == '"') return readString();
            if (c == 't' || c == 'f') return readBool();
            if (c == 'n') return readNull();
            return readNumber();
        }

        Map<String, Object> readObject() {
            pos++; // consume '{'
            Map<String, Object> m = new LinkedHashMap<>();
            skipWs();
            if (pos < src.length() && src.charAt(pos) == '}') {
                pos++;
                return m;
            }
            while (true) {
                skipWs();
                String k = readString();
                skipWs();
                if (pos >= src.length() || src.charAt(pos) != ':') {
                    throw new RuntimeException("expected ':' at " + pos);
                }
                pos++;
                Object v = readValue();
                m.put(k, v);
                skipWs();
                if (pos >= src.length()) throw new RuntimeException("eof in object");
                char c = src.charAt(pos);
                if (c == ',') { pos++; continue; }
                if (c == '}') { pos++; return m; }
                throw new RuntimeException("expected ',' or '}' at " + pos);
            }
        }

        List<Object> readArray() {
            pos++; // consume '['
            List<Object> a = new ArrayList<>();
            skipWs();
            if (pos < src.length() && src.charAt(pos) == ']') {
                pos++;
                return a;
            }
            while (true) {
                Object v = readValue();
                a.add(v);
                skipWs();
                if (pos >= src.length()) throw new RuntimeException("eof in array");
                char c = src.charAt(pos);
                if (c == ',') { pos++; continue; }
                if (c == ']') { pos++; return a; }
                throw new RuntimeException("expected ',' or ']' at " + pos);
            }
        }

        String readString() {
            if (pos >= src.length() || src.charAt(pos) != '"') {
                throw new RuntimeException("expected string at " + pos);
            }
            pos++;
            StringBuilder sb = new StringBuilder();
            while (pos < src.length()) {
                char c = src.charAt(pos++);
                if (c == '"') return sb.toString();
                if (c == '\\') {
                    if (pos >= src.length()) throw new RuntimeException("eof in escape");
                    char e = src.charAt(pos++);
                    switch (e) {
                        case '"':  sb.append('"');  break;
                        case '\\': sb.append('\\'); break;
                        case '/':  sb.append('/');  break;
                        case 'b':  sb.append('\b'); break;
                        case 'f':  sb.append('\f'); break;
                        case 'n':  sb.append('\n'); break;
                        case 'r':  sb.append('\r'); break;
                        case 't':  sb.append('\t'); break;
                        case 'u':
                            if (pos + 4 > src.length()) {
                                throw new RuntimeException("bad unicode escape");
                            }
                            sb.append((char) Integer.parseInt(src.substring(pos, pos + 4), 16));
                            pos += 4;
                            break;
                        default:
                            throw new RuntimeException("bad escape: \\" + e);
                    }
                } else {
                    sb.append(c);
                }
            }
            throw new RuntimeException("unterminated string");
        }

        Object readBool() {
            if (src.startsWith("true", pos))  { pos += 4; return Boolean.TRUE; }
            if (src.startsWith("false", pos)) { pos += 5; return Boolean.FALSE; }
            throw new RuntimeException("invalid literal at " + pos);
        }

        Object readNull() {
            if (src.startsWith("null", pos)) { pos += 4; return null; }
            throw new RuntimeException("invalid literal at " + pos);
        }

        Object readNumber() {
            int start = pos;
            if (pos < src.length() && src.charAt(pos) == '-') pos++;
            while (pos < src.length()) {
                char c = src.charAt(pos);
                if ((c >= '0' && c <= '9') || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-') {
                    pos++;
                } else break;
            }
            String num = src.substring(start, pos);
            if (num.indexOf('.') < 0 && num.indexOf('e') < 0 && num.indexOf('E') < 0) {
                try { return Long.parseLong(num); }
                catch (NumberFormatException e) { /* fall through */ }
            }
            return Double.parseDouble(num);
        }
    }
}
