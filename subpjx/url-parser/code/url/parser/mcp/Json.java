package url.parser.mcp;

import java.util.*;

/** Minimal JSON parser and serializer for the MCP wire protocol. */
final class Json {

    private Json() {}

    // ---- Parser ----

    static Object parse(String text) {
        Cursor c = new Cursor(text.trim());
        Object v = parseValue(c);
        c.skipWs();
        if (c.pos < c.s.length())
            throw new JsonException("trailing content at pos " + c.pos);
        return v;
    }

    private static Object parseValue(Cursor c) {
        c.skipWs();
        if (c.pos >= c.s.length()) throw new JsonException("unexpected end of input");
        char ch = c.s.charAt(c.pos);
        if (ch == '{') return parseObject(c);
        if (ch == '[') return parseArray(c);
        if (ch == '"') return parseString(c);
        if (ch == 't' || ch == 'f') return parseBoolean(c);
        if (ch == 'n') { parseNull(c); return null; }
        if (ch == '-' || Character.isDigit(ch)) return parseNumber(c);
        throw new JsonException("unexpected char '" + ch + "' at pos " + c.pos);
    }

    @SuppressWarnings("unchecked")
    static Map<String, Object> parseObject(Cursor c) {
        c.pos++; // {
        Map<String, Object> m = new LinkedHashMap<>();
        c.skipWs();
        if (c.pos < c.s.length() && c.s.charAt(c.pos) == '}') { c.pos++; return m; }
        while (true) {
            c.skipWs();
            if (c.s.charAt(c.pos) != '"') throw new JsonException("expected string key at " + c.pos);
            String key = parseString(c);
            c.skipWs();
            if (c.s.charAt(c.pos) != ':') throw new JsonException("expected ':' at " + c.pos);
            c.pos++;
            Object val = parseValue(c);
            m.put(key, val);
            c.skipWs();
            char sep = c.s.charAt(c.pos);
            if (sep == '}') { c.pos++; return m; }
            if (sep == ',') { c.pos++; }
            else throw new JsonException("expected '}' or ',' at " + c.pos);
        }
    }

    private static List<Object> parseArray(Cursor c) {
        c.pos++; // [
        List<Object> list = new ArrayList<>();
        c.skipWs();
        if (c.pos < c.s.length() && c.s.charAt(c.pos) == ']') { c.pos++; return list; }
        while (true) {
            list.add(parseValue(c));
            c.skipWs();
            char sep = c.s.charAt(c.pos);
            if (sep == ']') { c.pos++; return list; }
            if (sep == ',') { c.pos++; }
            else throw new JsonException("expected ']' or ',' at " + c.pos);
        }
    }

    static String parseString(Cursor c) {
        c.pos++; // "
        StringBuilder sb = new StringBuilder();
        while (c.pos < c.s.length()) {
            char ch = c.s.charAt(c.pos++);
            if (ch == '"') return sb.toString();
            if (ch == '\\') {
                char esc = c.s.charAt(c.pos++);
                switch (esc) {
                    case '"': sb.append('"'); break;
                    case '\\': sb.append('\\'); break;
                    case '/': sb.append('/'); break;
                    case 'b': sb.append('\b'); break;
                    case 'f': sb.append('\f'); break;
                    case 'n': sb.append('\n'); break;
                    case 'r': sb.append('\r'); break;
                    case 't': sb.append('\t'); break;
                    case 'u': {
                        String hex = c.s.substring(c.pos, c.pos + 4);
                        sb.append((char) Integer.parseInt(hex, 16));
                        c.pos += 4;
                        break;
                    }
                    default: throw new JsonException("invalid escape \\" + esc);
                }
            } else {
                sb.append(ch);
            }
        }
        throw new JsonException("unterminated string");
    }

    private static Number parseNumber(Cursor c) {
        int start = c.pos;
        if (c.s.charAt(c.pos) == '-') c.pos++;
        while (c.pos < c.s.length() && Character.isDigit(c.s.charAt(c.pos))) c.pos++;
        boolean isFloat = false;
        if (c.pos < c.s.length() && c.s.charAt(c.pos) == '.') {
            isFloat = true; c.pos++;
            while (c.pos < c.s.length() && Character.isDigit(c.s.charAt(c.pos))) c.pos++;
        }
        if (c.pos < c.s.length() && (c.s.charAt(c.pos) == 'e' || c.s.charAt(c.pos) == 'E')) {
            isFloat = true; c.pos++;
            if (c.pos < c.s.length() && (c.s.charAt(c.pos) == '+' || c.s.charAt(c.pos) == '-')) c.pos++;
            while (c.pos < c.s.length() && Character.isDigit(c.s.charAt(c.pos))) c.pos++;
        }
        String num = c.s.substring(start, c.pos);
        return isFloat ? Double.parseDouble(num) : Long.parseLong(num);
    }

    private static boolean parseBoolean(Cursor c) {
        if (c.s.startsWith("true", c.pos)) { c.pos += 4; return true; }
        if (c.s.startsWith("false", c.pos)) { c.pos += 5; return false; }
        throw new JsonException("invalid boolean at " + c.pos);
    }

    private static void parseNull(Cursor c) {
        if (c.s.startsWith("null", c.pos)) { c.pos += 4; return; }
        throw new JsonException("invalid null at " + c.pos);
    }

    static final class Cursor {
        final String s;
        int pos;
        Cursor(String s) { this.s = s; }
        void skipWs() { while (pos < s.length() && s.charAt(pos) <= ' ') pos++; }
    }

    // ---- Serializer ----

    static String write(Object value) {
        StringBuilder sb = new StringBuilder();
        writeValue(value, sb);
        return sb.toString();
    }

    @SuppressWarnings("unchecked")
    private static void writeValue(Object v, StringBuilder sb) {
        if (v == null) { sb.append("null"); return; }
        if (v instanceof String s) { writeString(s, sb); return; }
        if (v instanceof Boolean b) { sb.append(b); return; }
        if (v instanceof Number n) {
            if (n instanceof Long || n instanceof Integer) sb.append(n.longValue());
            else sb.append(n.doubleValue());
            return;
        }
        if (v instanceof Map<?, ?> map) {
            sb.append('{');
            boolean first = true;
            for (Map.Entry<?, ?> e : ((Map<?, ?>) map).entrySet()) {
                if (!first) sb.append(',');
                writeString(e.getKey().toString(), sb);
                sb.append(':');
                writeValue(e.getValue(), sb);
                first = false;
            }
            sb.append('}');
            return;
        }
        if (v instanceof List<?> list) {
            sb.append('[');
            boolean first = true;
            for (Object item : list) {
                if (!first) sb.append(',');
                writeValue(item, sb);
                first = false;
            }
            sb.append(']');
            return;
        }
        writeString(v.toString(), sb);
    }

    private static void writeString(String s, StringBuilder sb) {
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
                    if (c < 0x20) sb.append(String.format("\\u%04x", (int) c));
                    else sb.append(c);
            }
        }
        sb.append('"');
    }

    static final class JsonException extends RuntimeException {
        JsonException(String msg) { super(msg); }
    }
}
