package gitignore.matcher.mcp;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

final class Json {
    private Json() {}

    static Object parse(String text) {
        Parser parser = new Parser(text);
        Object value = parser.parseValue();
        parser.skipWhitespace();
        if (!parser.atEnd()) {
            throw new IllegalArgumentException("trailing input");
        }
        return value;
    }

    static String stringify(Object value) {
        StringBuilder out = new StringBuilder();
        write(out, value);
        return out.toString();
    }

    @SuppressWarnings("unchecked")
    private static void write(StringBuilder out, Object value) {
        if (value == null) {
            out.append("null");
        } else if (value instanceof String string) {
            writeString(out, string);
        } else if (value instanceof Number || value instanceof Boolean) {
            out.append(value);
        } else if (value instanceof Map<?, ?> map) {
            out.append('{');
            boolean first = true;
            List<String> keys = new ArrayList<>();
            for (Object key : map.keySet()) {
                keys.add((String) key);
            }
            keys.sort(Comparator.naturalOrder());
            for (String key : keys) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                writeString(out, key);
                out.append(':');
                write(out, ((Map<String, Object>) map).get(key));
            }
            out.append('}');
        } else if (value instanceof Iterable<?> iterable) {
            out.append('[');
            boolean first = true;
            for (Object item : iterable) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                write(out, item);
            }
            out.append(']');
        } else {
            throw new IllegalArgumentException("unsupported value: " + value.getClass().getName());
        }
    }

    private static void writeString(StringBuilder out, String value) {
        out.append('"');
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            switch (ch) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\b' -> out.append("\\b");
                case '\f' -> out.append("\\f");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (ch < 0x20) {
                        out.append(String.format("\\u%04x", (int) ch));
                    } else {
                        out.append(ch);
                    }
                }
            }
        }
        out.append('"');
    }

    private static final class Parser {
        private final String text;
        private int index;

        Parser(String text) {
            this.text = text;
        }

        boolean atEnd() {
            return index == text.length();
        }

        void skipWhitespace() {
            while (!atEnd()) {
                char ch = text.charAt(index);
                if (ch != ' ' && ch != '\n' && ch != '\r' && ch != '\t') {
                    return;
                }
                index++;
            }
        }

        Object parseValue() {
            skipWhitespace();
            if (atEnd()) {
                throw new IllegalArgumentException("unexpected end");
            }
            char ch = text.charAt(index);
            if (ch == '"') {
                return parseString();
            }
            if (ch == '{') {
                return parseObject();
            }
            if (ch == '[') {
                return parseArray();
            }
            if (ch == 't' && text.startsWith("true", index)) {
                index += 4;
                return Boolean.TRUE;
            }
            if (ch == 'f' && text.startsWith("false", index)) {
                index += 5;
                return Boolean.FALSE;
            }
            if (ch == 'n' && text.startsWith("null", index)) {
                index += 4;
                return null;
            }
            if (ch == '-' || Character.isDigit(ch)) {
                return parseNumber();
            }
            throw new IllegalArgumentException("unexpected character");
        }

        private Map<String, Object> parseObject() {
            index++;
            LinkedHashMap<String, Object> map = new LinkedHashMap<>();
            skipWhitespace();
            if (consume('}')) {
                return map;
            }
            while (true) {
                skipWhitespace();
                if (atEnd() || text.charAt(index) != '"') {
                    throw new IllegalArgumentException("object key must be string");
                }
                String key = parseString();
                skipWhitespace();
                expect(':');
                map.put(key, parseValue());
                skipWhitespace();
                if (consume('}')) {
                    return map;
                }
                expect(',');
            }
        }

        private List<Object> parseArray() {
            index++;
            ArrayList<Object> values = new ArrayList<>();
            skipWhitespace();
            if (consume(']')) {
                return values;
            }
            while (true) {
                values.add(parseValue());
                skipWhitespace();
                if (consume(']')) {
                    return values;
                }
                expect(',');
            }
        }

        private String parseString() {
            expect('"');
            StringBuilder out = new StringBuilder();
            while (!atEnd()) {
                char ch = text.charAt(index++);
                if (ch == '"') {
                    return out.toString();
                }
                if (ch == '\\') {
                    if (atEnd()) {
                        throw new IllegalArgumentException("bad escape");
                    }
                    char escaped = text.charAt(index++);
                    switch (escaped) {
                        case '"', '\\', '/' -> out.append(escaped);
                        case 'b' -> out.append('\b');
                        case 'f' -> out.append('\f');
                        case 'n' -> out.append('\n');
                        case 'r' -> out.append('\r');
                        case 't' -> out.append('\t');
                        case 'u' -> out.append(parseUnicode());
                        default -> throw new IllegalArgumentException("bad escape");
                    }
                } else {
                    out.append(ch);
                }
            }
            throw new IllegalArgumentException("unterminated string");
        }

        private char parseUnicode() {
            if (index + 4 > text.length()) {
                throw new IllegalArgumentException("short unicode escape");
            }
            int value = Integer.parseInt(text.substring(index, index + 4), 16);
            index += 4;
            return (char) value;
        }

        private Number parseNumber() {
            int start = index;
            if (consume('-') && atEnd()) {
                throw new IllegalArgumentException("bad number");
            }
            while (!atEnd() && Character.isDigit(text.charAt(index))) {
                index++;
            }
            if (!atEnd() && text.charAt(index) == '.') {
                index++;
                while (!atEnd() && Character.isDigit(text.charAt(index))) {
                    index++;
                }
                return Double.parseDouble(text.substring(start, index));
            }
            return Long.parseLong(text.substring(start, index));
        }

        private boolean consume(char expected) {
            if (!atEnd() && text.charAt(index) == expected) {
                index++;
                return true;
            }
            return false;
        }

        private void expect(char expected) {
            if (!consume(expected)) {
                throw new IllegalArgumentException("expected " + expected);
            }
        }
    }
}
