package snapshot.db;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

public final class Json {
    private Json() {}

    public static String stringify(Object v) {
        StringBuilder sb = new StringBuilder();
        write(sb, v);
        return sb.toString();
    }

    private static void write(StringBuilder sb, Object v) {
        if (v == null) {
            sb.append("null");
            return;
        }
        if (v instanceof Boolean b) {
            sb.append(b ? "true" : "false");
            return;
        }
        if (v instanceof Number n) {
            if (n instanceof Double d) {
                if (d.isNaN() || d.isInfinite()) sb.append("null");
                else if (d == Math.floor(d)) sb.append(Long.toString(d.longValue()));
                else sb.append(d.toString());
                return;
            }
            if (n instanceof Float f) {
                if (f.isNaN() || f.isInfinite()) sb.append("null");
                else if (f == Math.floor(f)) sb.append(Long.toString(f.longValue()));
                else sb.append(f.toString());
                return;
            }
            sb.append(n.toString());
            return;
        }
        if (v instanceof String s) {
            writeString(sb, s);
            return;
        }
        if (v instanceof Map<?, ?> m) {
            sb.append('{');
            TreeMap<String, Object> sorted = new TreeMap<>();
            for (Map.Entry<?, ?> e : m.entrySet()) {
                sorted.put(e.getKey().toString(), e.getValue());
            }
            boolean first = true;
            for (Map.Entry<String, Object> e : sorted.entrySet()) {
                if (!first) sb.append(',');
                first = false;
                writeString(sb, e.getKey());
                sb.append(':');
                write(sb, e.getValue());
            }
            sb.append('}');
            return;
        }
        if (v instanceof List<?> l) {
            sb.append('[');
            boolean first = true;
            for (Object e : l) {
                if (!first) sb.append(',');
                first = false;
                write(sb, e);
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
                case '"':  sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
                case '\b': sb.append("\\b"); break;
                case '\f': sb.append("\\f"); break;
                default:
                    if (c < 0x20) sb.append(String.format("\\u%04x", (int) c));
                    else sb.append(c);
            }
        }
        sb.append('"');
    }

    public static Object parse(String s) {
        Parser p = new Parser(s);
        p.skipWs();
        Object r = p.readValue();
        p.skipWs();
        if (p.pos < p.s.length()) {
            throw new IllegalArgumentException("trailing data at " + p.pos);
        }
        return r;
    }

    private static final class Parser {
        final String s;
        int pos;

        Parser(String s) { this.s = s; }

        void skipWs() {
            while (pos < s.length()) {
                char c = s.charAt(pos);
                if (c == ' ' || c == '\t' || c == '\n' || c == '\r') pos++;
                else break;
            }
        }

        Object readValue() {
            skipWs();
            if (pos >= s.length()) throw new IllegalArgumentException("unexpected eof");
            char c = s.charAt(pos);
            switch (c) {
                case '{': return readObject();
                case '[': return readArray();
                case '"': return readString();
                case 't':
                case 'f': return readBool();
                case 'n': return readNull();
                default:  return readNumber();
            }
        }

        Map<String, Object> readObject() {
            Map<String, Object> m = new LinkedHashMap<>();
            pos++;
            skipWs();
            if (pos < s.length() && s.charAt(pos) == '}') { pos++; return m; }
            while (true) {
                skipWs();
                String key = readString();
                skipWs();
                if (pos >= s.length() || s.charAt(pos) != ':')
                    throw new IllegalArgumentException("expected : at " + pos);
                pos++;
                Object v = readValue();
                m.put(key, v);
                skipWs();
                if (pos < s.length() && s.charAt(pos) == ',') { pos++; continue; }
                if (pos < s.length() && s.charAt(pos) == '}') { pos++; return m; }
                throw new IllegalArgumentException("expected , or } at " + pos);
            }
        }

        List<Object> readArray() {
            List<Object> l = new ArrayList<>();
            pos++;
            skipWs();
            if (pos < s.length() && s.charAt(pos) == ']') { pos++; return l; }
            while (true) {
                Object v = readValue();
                l.add(v);
                skipWs();
                if (pos < s.length() && s.charAt(pos) == ',') { pos++; continue; }
                if (pos < s.length() && s.charAt(pos) == ']') { pos++; return l; }
                throw new IllegalArgumentException("expected , or ] at " + pos);
            }
        }

        String readString() {
            if (pos >= s.length() || s.charAt(pos) != '"')
                throw new IllegalArgumentException("expected string at " + pos);
            pos++;
            StringBuilder sb = new StringBuilder();
            while (pos < s.length()) {
                char c = s.charAt(pos++);
                if (c == '"') return sb.toString();
                if (c == '\\') {
                    if (pos >= s.length()) throw new IllegalArgumentException("bad escape");
                    char e = s.charAt(pos++);
                    switch (e) {
                        case '"':  sb.append('"'); break;
                        case '\\': sb.append('\\'); break;
                        case '/':  sb.append('/'); break;
                        case 'n':  sb.append('\n'); break;
                        case 'r':  sb.append('\r'); break;
                        case 't':  sb.append('\t'); break;
                        case 'b':  sb.append('\b'); break;
                        case 'f':  sb.append('\f'); break;
                        case 'u':
                            if (pos + 4 > s.length())
                                throw new IllegalArgumentException("bad unicode");
                            int cp = Integer.parseInt(s.substring(pos, pos + 4), 16);
                            pos += 4;
                            sb.append((char) cp);
                            break;
                        default:
                            throw new IllegalArgumentException("unknown escape \\" + e);
                    }
                } else {
                    sb.append(c);
                }
            }
            throw new IllegalArgumentException("unterminated string");
        }

        Object readBool() {
            if (s.startsWith("true", pos))  { pos += 4; return Boolean.TRUE;  }
            if (s.startsWith("false", pos)) { pos += 5; return Boolean.FALSE; }
            throw new IllegalArgumentException("expected bool at " + pos);
        }

        Object readNull() {
            if (s.startsWith("null", pos)) { pos += 4; return null; }
            throw new IllegalArgumentException("expected null at " + pos);
        }

        Object readNumber() {
            int start = pos;
            if (s.charAt(pos) == '-') pos++;
            while (pos < s.length()) {
                char c = s.charAt(pos);
                if (Character.isDigit(c) || c == '.' || c == 'e' || c == 'E'
                        || c == '+' || c == '-') {
                    pos++;
                } else break;
            }
            String t = s.substring(start, pos);
            if (t.contains(".") || t.contains("e") || t.contains("E"))
                return Double.parseDouble(t);
            return Long.parseLong(t);
        }
    }
}
