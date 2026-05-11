package sftp.protocol.mcp;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class Json {
    private Json() {}

    public static String stringify(Object o) {
        StringBuilder sb = new StringBuilder();
        write(sb, o);
        return sb.toString();
    }

    private static void write(StringBuilder sb, Object o) {
        if (o == null) { sb.append("null"); return; }
        if (o instanceof Boolean b) { sb.append(b ? "true" : "false"); return; }
        if (o instanceof Number n) {
            if (n instanceof Double || n instanceof Float) {
                double d = n.doubleValue();
                if (d == Math.floor(d) && !Double.isInfinite(d)) {
                    sb.append((long) d);
                } else {
                    sb.append(d);
                }
            } else {
                sb.append(n.toString());
            }
            return;
        }
        if (o instanceof CharSequence s) { writeString(sb, s.toString()); return; }
        if (o instanceof Map<?, ?> m) {
            sb.append('{');
            boolean first = true;
            for (Map.Entry<?, ?> e : m.entrySet()) {
                if (!first) sb.append(',');
                first = false;
                writeString(sb, String.valueOf(e.getKey()));
                sb.append(':');
                write(sb, e.getValue());
            }
            sb.append('}');
            return;
        }
        if (o instanceof Iterable<?> it) {
            sb.append('[');
            boolean first = true;
            for (Object e : it) {
                if (!first) sb.append(',');
                first = false;
                write(sb, e);
            }
            sb.append(']');
            return;
        }
        if (o instanceof byte[] bytes) {
            writeString(sb, java.util.Base64.getEncoder().encodeToString(bytes));
            return;
        }
        writeString(sb, String.valueOf(o));
    }

    private static void writeString(StringBuilder sb, String s) {
        sb.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"': sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\b': sb.append("\\b"); break;
                case '\f': sb.append("\\f"); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
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

    public static Object parse(String s) {
        Parser p = new Parser(s);
        p.skipWs();
        Object v = p.readValue();
        p.skipWs();
        return v;
    }

    private static final class Parser {
        final String src;
        int pos;

        Parser(String src) { this.src = src; this.pos = 0; }

        void skipWs() {
            while (pos < src.length()) {
                char c = src.charAt(pos);
                if (c == ' ' || c == '\t' || c == '\n' || c == '\r') pos++;
                else break;
            }
        }

        Object readValue() {
            skipWs();
            if (pos >= src.length()) throw new RuntimeException("unexpected EOF");
            char c = src.charAt(pos);
            if (c == '{') return readObject();
            if (c == '[') return readArray();
            if (c == '"') return readString();
            if (c == 't' || c == 'f') return readBool();
            if (c == 'n') return readNull();
            return readNumber();
        }

        Map<String, Object> readObject() {
            Map<String, Object> m = new LinkedHashMap<>();
            pos++; // {
            skipWs();
            if (pos < src.length() && src.charAt(pos) == '}') { pos++; return m; }
            while (true) {
                skipWs();
                String k = readString();
                skipWs();
                if (pos >= src.length() || src.charAt(pos) != ':') throw new RuntimeException("expected ':'");
                pos++;
                Object v = readValue();
                m.put(k, v);
                skipWs();
                if (pos >= src.length()) throw new RuntimeException("unexpected EOF in object");
                char c = src.charAt(pos);
                if (c == ',') { pos++; continue; }
                if (c == '}') { pos++; return m; }
                throw new RuntimeException("expected ',' or '}' at " + pos);
            }
        }

        List<Object> readArray() {
            List<Object> list = new ArrayList<>();
            pos++; // [
            skipWs();
            if (pos < src.length() && src.charAt(pos) == ']') { pos++; return list; }
            while (true) {
                Object v = readValue();
                list.add(v);
                skipWs();
                if (pos >= src.length()) throw new RuntimeException("unexpected EOF in array");
                char c = src.charAt(pos);
                if (c == ',') { pos++; continue; }
                if (c == ']') { pos++; return list; }
                throw new RuntimeException("expected ',' or ']' at " + pos);
            }
        }

        String readString() {
            if (src.charAt(pos) != '"') throw new RuntimeException("expected '\"' at " + pos);
            pos++;
            StringBuilder sb = new StringBuilder();
            while (pos < src.length()) {
                char c = src.charAt(pos++);
                if (c == '"') return sb.toString();
                if (c == '\\') {
                    if (pos >= src.length()) throw new RuntimeException("EOF in escape");
                    char esc = src.charAt(pos++);
                    switch (esc) {
                        case '"': sb.append('"'); break;
                        case '\\': sb.append('\\'); break;
                        case '/': sb.append('/'); break;
                        case 'b': sb.append('\b'); break;
                        case 'f': sb.append('\f'); break;
                        case 'n': sb.append('\n'); break;
                        case 'r': sb.append('\r'); break;
                        case 't': sb.append('\t'); break;
                        case 'u':
                            if (pos + 4 > src.length()) throw new RuntimeException("bad \\u");
                            sb.append((char) Integer.parseInt(src.substring(pos, pos + 4), 16));
                            pos += 4;
                            break;
                        default: throw new RuntimeException("bad escape: " + esc);
                    }
                } else {
                    sb.append(c);
                }
            }
            throw new RuntimeException("EOF in string");
        }

        Boolean readBool() {
            if (src.startsWith("true", pos)) { pos += 4; return Boolean.TRUE; }
            if (src.startsWith("false", pos)) { pos += 5; return Boolean.FALSE; }
            throw new RuntimeException("bad bool at " + pos);
        }

        Object readNull() {
            if (src.startsWith("null", pos)) { pos += 4; return null; }
            throw new RuntimeException("bad null at " + pos);
        }

        Number readNumber() {
            int start = pos;
            if (src.charAt(pos) == '-') pos++;
            while (pos < src.length() && Character.isDigit(src.charAt(pos))) pos++;
            boolean isDouble = false;
            if (pos < src.length() && src.charAt(pos) == '.') {
                isDouble = true;
                pos++;
                while (pos < src.length() && Character.isDigit(src.charAt(pos))) pos++;
            }
            if (pos < src.length() && (src.charAt(pos) == 'e' || src.charAt(pos) == 'E')) {
                isDouble = true;
                pos++;
                if (pos < src.length() && (src.charAt(pos) == '+' || src.charAt(pos) == '-')) pos++;
                while (pos < src.length() && Character.isDigit(src.charAt(pos))) pos++;
            }
            String s = src.substring(start, pos);
            if (isDouble) return Double.parseDouble(s);
            try { return Long.parseLong(s); }
            catch (NumberFormatException e) { return Double.parseDouble(s); }
        }
    }
}
