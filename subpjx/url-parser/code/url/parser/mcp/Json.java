package url.parser.mcp;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

final class Json {
    private final String text;
    private int index;

    private Json(String text) {
        this.text = text;
    }

    static Object parse(String text) {
        Json parser = new Json(text);
        Object value = parser.value();
        parser.space();
        if (parser.index != text.length()) {
            throw new JsonException("trailing input");
        }
        return value;
    }

    static String stringify(Object value) {
        StringBuilder out = new StringBuilder();
        write(value, out);
        return out.toString();
    }

    private Object value() {
        space();
        if (index >= text.length()) {
            throw new JsonException("empty input");
        }
        char c = text.charAt(index);
        if (c == '"') {
            return string();
        }
        if (c == '{') {
            return object();
        }
        if (c == '[') {
            return array();
        }
        if (c == 't' && take("true")) {
            return Boolean.TRUE;
        }
        if (c == 'f' && take("false")) {
            return Boolean.FALSE;
        }
        if (c == 'n' && take("null")) {
            return null;
        }
        if (c == '-' || Character.isDigit(c)) {
            return number();
        }
        throw new JsonException("unexpected character");
    }

    private Map<String, Object> object() {
        expect('{');
        Map<String, Object> out = new LinkedHashMap<>();
        space();
        if (peek('}')) {
            index++;
            return out;
        }
        while (true) {
            space();
            String key = string();
            space();
            expect(':');
            out.put(key, value());
            space();
            if (peek('}')) {
                index++;
                return out;
            }
            expect(',');
        }
    }

    private List<Object> array() {
        expect('[');
        List<Object> out = new ArrayList<>();
        space();
        if (peek(']')) {
            index++;
            return out;
        }
        while (true) {
            out.add(value());
            space();
            if (peek(']')) {
                index++;
                return out;
            }
            expect(',');
        }
    }

    private String string() {
        expect('"');
        StringBuilder out = new StringBuilder();
        while (index < text.length()) {
            char c = text.charAt(index++);
            if (c == '"') {
                return out.toString();
            }
            if (c == '\\') {
                if (index >= text.length()) {
                    throw new JsonException("bad escape");
                }
                char e = text.charAt(index++);
                switch (e) {
                    case '"' -> out.append('"');
                    case '\\' -> out.append('\\');
                    case '/' -> out.append('/');
                    case 'b' -> out.append('\b');
                    case 'f' -> out.append('\f');
                    case 'n' -> out.append('\n');
                    case 'r' -> out.append('\r');
                    case 't' -> out.append('\t');
                    case 'u' -> out.append((char) unicode());
                    default -> throw new JsonException("bad escape");
                }
            } else {
                out.append(c);
            }
        }
        throw new JsonException("unterminated string");
    }

    private int unicode() {
        if (index + 4 > text.length()) {
            throw new JsonException("bad unicode escape");
        }
        int value = 0;
        for (int i = 0; i < 4; i++) {
            int digit = Character.digit(text.charAt(index++), 16);
            if (digit < 0) {
                throw new JsonException("bad unicode escape");
            }
            value = value * 16 + digit;
        }
        return value;
    }

    private Number number() {
        int start = index;
        if (peek('-')) {
            index++;
        }
        while (index < text.length() && Character.isDigit(text.charAt(index))) {
            index++;
        }
        if (index < text.length() && text.charAt(index) == '.') {
            index++;
            while (index < text.length() && Character.isDigit(text.charAt(index))) {
                index++;
            }
            return Double.parseDouble(text.substring(start, index));
        }
        return Long.parseLong(text.substring(start, index));
    }

    private void space() {
        while (index < text.length() && Character.isWhitespace(text.charAt(index))) {
            index++;
        }
    }

    private boolean take(String value) {
        if (text.startsWith(value, index)) {
            index += value.length();
            return true;
        }
        return false;
    }

    private boolean peek(char c) {
        return index < text.length() && text.charAt(index) == c;
    }

    private void expect(char c) {
        if (!peek(c)) {
            throw new JsonException("expected " + c);
        }
        index++;
    }

    @SuppressWarnings("unchecked")
    private static void write(Object value, StringBuilder out) {
        if (value == null) {
            out.append("null");
        } else if (value instanceof String text) {
            string(text, out);
        } else if (value instanceof Number || value instanceof Boolean) {
            out.append(value);
        } else if (value instanceof Map<?, ?> map) {
            out.append('{');
            boolean first = true;
            for (Map.Entry<String, Object> entry : ((Map<String, Object>) map).entrySet()) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                string(entry.getKey(), out);
                out.append(':');
                write(entry.getValue(), out);
            }
            out.append('}');
        } else if (value instanceof Iterable<?> values) {
            out.append('[');
            boolean first = true;
            for (Object item : values) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                write(item, out);
            }
            out.append(']');
        } else {
            throw new IllegalArgumentException("unsupported JSON value");
        }
    }

    private static void string(String value, StringBuilder out) {
        out.append('"');
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\b' -> out.append("\\b");
                case '\f' -> out.append("\\f");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (c < 0x20) {
                        out.append(String.format("\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        out.append('"');
    }
}
