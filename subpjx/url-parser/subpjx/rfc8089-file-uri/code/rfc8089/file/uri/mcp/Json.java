package rfc8089.file.uri.mcp;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

final class Json {

    private Json() {}

    static Object parse(String s) {
        Parser p = new Parser(s);
        p.skipWs();
        Object v = p.parseValue();
        p.skipWs();
        if (p.pos < p.s.length()) throw new RuntimeException("trailing data");
        return v;
    }

    static String stringify(Object v) {
        StringBuilder sb = new StringBuilder();
        write(sb, v);
        return sb.toString();
    }

    private static void write(StringBuilder sb, Object v) {
        if (v == null) { sb.append("null"); return; }
        if (v instanceof Boolean) {
            sb.append(((Boolean) v) ? "true" : "false");
            return;
        }
        if (v instanceof Number) {
            Number n = (Number) v;
            if (n instanceof Integer || n instanceof Long || n instanceof Short || n instanceof Byte) {
                sb.append(n.toString());
            } else {
                double d = n.doubleValue();
                if (Double.isFinite(d) && d == Math.floor(d) && !Double.isInfinite(d)
                        && d >= Long.MIN_VALUE && d <= Long.MAX_VALUE) {
                    sb.append((long) d);
                } else {
                    sb.append(d);
                }
            }
            return;
        }
        if (v instanceof String) {
            writeString(sb, (String) v);
            return;
        }
        if (v instanceof List) {
            sb.append('[');
            boolean first = true;
            for (Object e : (List<?>) v) {
                if (!first) sb.append(',');
                first = false;
                write(sb, e);
            }
            sb.append(']');
            return;
        }
        if (v instanceof Map) {
            Map<?, ?> m = (Map<?, ?>) v;
            List<String> keys = new ArrayList<>(m.size());
            for (Object k : m.keySet()) keys.add(String.valueOf(k));
            Collections.sort(keys);
            sb.append('{');
            boolean first = true;
            for (String k : keys) {
                if (!first) sb.append(',');
                first = false;
                writeString(sb, k);
                sb.append(':');
                write(sb, m.get(k));
            }
            sb.append('}');
            return;
        }
        throw new RuntimeException("cannot serialize: " + v.getClass());
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

    private static class Parser {
        final String s;
        int pos;

        Parser(String s) { this.s = s; }

        void skipWs() {
            while (pos < s.length() && Character.isWhitespace(s.charAt(pos))) pos++;
        }

        Object parseValue() {
            skipWs();
            if (pos >= s.length()) throw new RuntimeException("unexpected end");
            char c = s.charAt(pos);
            if (c == '{') return parseObject();
            if (c == '[') return parseArray();
            if (c == '"') return parseString();
            if (c == 't' || c == 'f') return parseBool();
            if (c == 'n') return parseNull();
            return parseNumber();
        }

        Map<String, Object> parseObject() {
            LinkedHashMap<String, Object> m = new LinkedHashMap<>();
            pos++;
            skipWs();
            if (pos < s.length() && s.charAt(pos) == '}') { pos++; return m; }
            while (true) {
                skipWs();
                String k = parseString();
                skipWs();
                if (pos >= s.length() || s.charAt(pos) != ':') throw new RuntimeException("expected :");
                pos++;
                Object v = parseValue();
                m.put(k, v);
                skipWs();
                if (pos < s.length() && s.charAt(pos) == ',') { pos++; continue; }
                if (pos < s.length() && s.charAt(pos) == '}') { pos++; return m; }
                throw new RuntimeException("expected , or }");
            }
        }

        List<Object> parseArray() {
            List<Object> a = new ArrayList<>();
            pos++;
            skipWs();
            if (pos < s.length() && s.charAt(pos) == ']') { pos++; return a; }
            while (true) {
                a.add(parseValue());
                skipWs();
                if (pos < s.length() && s.charAt(pos) == ',') { pos++; continue; }
                if (pos < s.length() && s.charAt(pos) == ']') { pos++; return a; }
                throw new RuntimeException("expected , or ]");
            }
        }

        String parseString() {
            if (pos >= s.length() || s.charAt(pos) != '"') throw new RuntimeException("expected \"");
            pos++;
            StringBuilder sb = new StringBuilder();
            while (pos < s.length()) {
                char c = s.charAt(pos);
                if (c == '"') { pos++; return sb.toString(); }
                if (c == '\\') {
                    pos++;
                    if (pos >= s.length()) throw new RuntimeException("bad escape");
                    char e = s.charAt(pos++);
                    switch (e) {
                        case '"': sb.append('"'); break;
                        case '\\': sb.append('\\'); break;
                        case '/': sb.append('/'); break;
                        case 'b': sb.append('\b'); break;
                        case 'f': sb.append('\f'); break;
                        case 'n': sb.append('\n'); break;
                        case 'r': sb.append('\r'); break;
                        case 't': sb.append('\t'); break;
                        case 'u':
                            if (pos + 4 > s.length()) throw new RuntimeException("bad unicode");
                            int code = Integer.parseInt(s.substring(pos, pos + 4), 16);
                            sb.append((char) code);
                            pos += 4;
                            break;
                        default: throw new RuntimeException("bad escape: " + e);
                    }
                } else {
                    sb.append(c);
                    pos++;
                }
            }
            throw new RuntimeException("unterminated string");
        }

        Boolean parseBool() {
            if (s.startsWith("true", pos)) { pos += 4; return Boolean.TRUE; }
            if (s.startsWith("false", pos)) { pos += 5; return Boolean.FALSE; }
            throw new RuntimeException("bad bool");
        }

        Object parseNull() {
            if (s.startsWith("null", pos)) { pos += 4; return null; }
            throw new RuntimeException("bad null");
        }

        Number parseNumber() {
            int start = pos;
            if (s.charAt(pos) == '-') pos++;
            while (pos < s.length() && Character.isDigit(s.charAt(pos))) pos++;
            boolean isFloat = false;
            if (pos < s.length() && s.charAt(pos) == '.') {
                isFloat = true;
                pos++;
                while (pos < s.length() && Character.isDigit(s.charAt(pos))) pos++;
            }
            if (pos < s.length() && (s.charAt(pos) == 'e' || s.charAt(pos) == 'E')) {
                isFloat = true;
                pos++;
                if (pos < s.length() && (s.charAt(pos) == '+' || s.charAt(pos) == '-')) pos++;
                while (pos < s.length() && Character.isDigit(s.charAt(pos))) pos++;
            }
            String num = s.substring(start, pos);
            if (isFloat) return Double.parseDouble(num);
            try {
                return Long.parseLong(num);
            } catch (NumberFormatException e) {
                return Double.parseDouble(num);
            }
        }
    }
}
