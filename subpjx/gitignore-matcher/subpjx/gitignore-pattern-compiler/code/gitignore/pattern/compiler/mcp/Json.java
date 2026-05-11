package gitignore.pattern.compiler.mcp;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class Json {

    private Json() {}

    public static Object parse(String s) {
        Parser p = new Parser(s);
        Object v = p.parseValue();
        p.skipWs();
        if (p.pos < p.text.length()) {
            throw new RuntimeException("trailing content at " + p.pos);
        }
        return v;
    }

    public static String emit(Object v) {
        StringBuilder sb = new StringBuilder();
        emit(v, sb);
        return sb.toString();
    }

    private static void emit(Object v, StringBuilder sb) {
        if (v == null) {
            sb.append("null");
        } else if (v instanceof Boolean b) {
            sb.append(b ? "true" : "false");
        } else if (v instanceof Number n) {
            if (n instanceof Integer || n instanceof Long || n instanceof Short || n instanceof Byte) {
                sb.append(n.toString());
            } else {
                double d = n.doubleValue();
                if (d == Math.floor(d) && !Double.isInfinite(d) && Math.abs(d) < 1e15) {
                    sb.append(Long.toString((long) d));
                } else {
                    sb.append(Double.toString(d));
                }
            }
        } else if (v instanceof String s) {
            emitString(s, sb);
        } else if (v instanceof Map<?, ?> m) {
            sb.append('{');
            boolean first = true;
            for (Map.Entry<?, ?> e : m.entrySet()) {
                if (!first) sb.append(',');
                emitString(e.getKey().toString(), sb);
                sb.append(':');
                emit(e.getValue(), sb);
                first = false;
            }
            sb.append('}');
        } else if (v instanceof List<?> list) {
            sb.append('[');
            boolean first = true;
            for (Object item : list) {
                if (!first) sb.append(',');
                emit(item, sb);
                first = false;
            }
            sb.append(']');
        } else {
            throw new RuntimeException("cannot emit: " + v.getClass());
        }
    }

    private static void emitString(String s, StringBuilder sb) {
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

    private static final class Parser {
        final String text;
        int pos;

        Parser(String text) {
            this.text = text;
            this.pos = 0;
        }

        void skipWs() {
            while (pos < text.length()) {
                char c = text.charAt(pos);
                if (c == ' ' || c == '\t' || c == '\n' || c == '\r') pos++;
                else break;
            }
        }

        Object parseValue() {
            skipWs();
            if (pos >= text.length()) throw new RuntimeException("unexpected end");
            char c = text.charAt(pos);
            if (c == '{') return parseObject();
            if (c == '[') return parseArray();
            if (c == '"') return parseString();
            if (c == 't' || c == 'f') return parseBool();
            if (c == 'n') return parseNull();
            if (c == '-' || (c >= '0' && c <= '9')) return parseNumber();
            throw new RuntimeException("unexpected char at " + pos + ": " + c);
        }

        Map<String, Object> parseObject() {
            Map<String, Object> m = new LinkedHashMap<>();
            pos++;
            skipWs();
            if (pos < text.length() && text.charAt(pos) == '}') {
                pos++;
                return m;
            }
            while (true) {
                skipWs();
                String key = parseString();
                skipWs();
                if (pos >= text.length() || text.charAt(pos) != ':') throw new RuntimeException("expected ':'");
                pos++;
                Object value = parseValue();
                m.put(key, value);
                skipWs();
                if (pos >= text.length()) throw new RuntimeException("expected '}' or ','");
                char c = text.charAt(pos);
                if (c == ',') { pos++; continue; }
                if (c == '}') { pos++; return m; }
                throw new RuntimeException("expected '}' or ',' at " + pos);
            }
        }

        List<Object> parseArray() {
            List<Object> list = new ArrayList<>();
            pos++;
            skipWs();
            if (pos < text.length() && text.charAt(pos) == ']') {
                pos++;
                return list;
            }
            while (true) {
                list.add(parseValue());
                skipWs();
                if (pos >= text.length()) throw new RuntimeException("expected ']' or ','");
                char c = text.charAt(pos);
                if (c == ',') { pos++; continue; }
                if (c == ']') { pos++; return list; }
                throw new RuntimeException("expected ']' or ',' at " + pos);
            }
        }

        String parseString() {
            if (pos >= text.length() || text.charAt(pos) != '"') throw new RuntimeException("expected '\"' at " + pos);
            pos++;
            StringBuilder sb = new StringBuilder();
            while (pos < text.length()) {
                char c = text.charAt(pos);
                if (c == '"') { pos++; return sb.toString(); }
                if (c == '\\') {
                    pos++;
                    if (pos >= text.length()) throw new RuntimeException("unterminated escape");
                    char esc = text.charAt(pos);
                    pos++;
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
                            if (pos + 4 > text.length()) throw new RuntimeException("bad \\u escape");
                            int code = Integer.parseInt(text.substring(pos, pos + 4), 16);
                            sb.append((char) code);
                            pos += 4;
                            break;
                        default:
                            throw new RuntimeException("bad escape: \\" + esc);
                    }
                } else {
                    sb.append(c);
                    pos++;
                }
            }
            throw new RuntimeException("unterminated string");
        }

        Boolean parseBool() {
            if (text.startsWith("true", pos)) { pos += 4; return Boolean.TRUE; }
            if (text.startsWith("false", pos)) { pos += 5; return Boolean.FALSE; }
            throw new RuntimeException("bad bool at " + pos);
        }

        Object parseNull() {
            if (text.startsWith("null", pos)) { pos += 4; return null; }
            throw new RuntimeException("bad null at " + pos);
        }

        Number parseNumber() {
            int start = pos;
            if (text.charAt(pos) == '-') pos++;
            while (pos < text.length() && text.charAt(pos) >= '0' && text.charAt(pos) <= '9') pos++;
            boolean isFloat = false;
            if (pos < text.length() && text.charAt(pos) == '.') {
                isFloat = true;
                pos++;
                while (pos < text.length() && text.charAt(pos) >= '0' && text.charAt(pos) <= '9') pos++;
            }
            if (pos < text.length() && (text.charAt(pos) == 'e' || text.charAt(pos) == 'E')) {
                isFloat = true;
                pos++;
                if (pos < text.length() && (text.charAt(pos) == '+' || text.charAt(pos) == '-')) pos++;
                while (pos < text.length() && text.charAt(pos) >= '0' && text.charAt(pos) <= '9') pos++;
            }
            String numStr = text.substring(start, pos);
            if (isFloat) return Double.parseDouble(numStr);
            try {
                return Integer.parseInt(numStr);
            } catch (NumberFormatException e) {
                return Long.parseLong(numStr);
            }
        }
    }
}
