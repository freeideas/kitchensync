package gitignore.pattern.syntax.mcp;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

final class MiniJson {

    private MiniJson() {}

    static Object parse(String input) {
        return new Parser(input).parse();
    }

    static String stringify(Object value) {
        return new Writer().write(value);
    }

    private static final class Parser {
        private final String input;
        private int index;

        Parser(String input) {
            this.input = input == null ? "" : input;
            this.index = 0;
        }

        Object parse() {
            Object value = readValue();
            skipWhitespace();
            if (index != input.length()) {
                throw new IllegalArgumentException("trailing input");
            }
            return value;
        }

        private Object readValue() {
            skipWhitespace();
            if (index >= input.length()) {
                throw new IllegalArgumentException("unexpected end");
            }

            char c = input.charAt(index);
            if (c == '{') {
                return readObject();
            }
            if (c == '[') {
                return readArray();
            }
            if (c == '"') {
                return readString();
            }
            if (c == 't' || c == 'f' || c == 'n') {
                return readKeyword();
            }
            if (c == '-' || (c >= '0' && c <= '9')) {
                return readNumber();
            }

            throw new IllegalArgumentException("invalid token at " + index);
        }

        private Map<String, Object> readObject() {
            expect('{');
            Map<String, Object> obj = new LinkedHashMap<>();
            skipWhitespace();
            if (peek() == '}') {
                index++;
                return obj;
            }
            while (true) {
                String key = readString();
                skipWhitespace();
                expect(':');
                Object value = readValue();
                obj.put(key, value);
                skipWhitespace();
                char next = peek();
                if (next == ',') {
                    index++;
                    continue;
                }
                if (next == '}') {
                    index++;
                    return obj;
                }
                throw new IllegalArgumentException("invalid object at " + index);
            }
        }

        private List<Object> readArray() {
            expect('[');
            List<Object> array = new ArrayList<>();
            skipWhitespace();
            if (peek() == ']') {
                index++;
                return array;
            }
            while (true) {
                array.add(readValue());
                skipWhitespace();
                char next = peek();
                if (next == ',') {
                    index++;
                    continue;
                }
                if (next == ']') {
                    index++;
                    return array;
                }
                throw new IllegalArgumentException("invalid array at " + index);
            }
        }

        private Object readKeyword() {
            if (input.startsWith("true", index)) {
                index += 4;
                return true;
            }
            if (input.startsWith("false", index)) {
                index += 5;
                return false;
            }
            if (input.startsWith("null", index)) {
                index += 4;
                return null;
            }
            throw new IllegalArgumentException("invalid keyword at " + index);
        }

        private Object readNumber() {
            int start = index;
            if (peek() == '-') {
                index++;
            }
            while (index < input.length() && Character.isDigit(input.charAt(index))) {
                index++;
            }
            if (index < input.length() && input.charAt(index) == '.') {
                index++;
                while (index < input.length() && Character.isDigit(input.charAt(index))) {
                    index++;
                }
            }
            if (index < input.length() && (input.charAt(index) == 'e' || input.charAt(index) == 'E')) {
                index++;
                if (index < input.length() && (input.charAt(index) == '+' || input.charAt(index) == '-')) {
                    index++;
                }
                while (index < input.length() && Character.isDigit(input.charAt(index))) {
                    index++;
                }
            }

            String token = input.substring(start, index);
            if (token.indexOf('.') >= 0 || token.indexOf('e') >= 0 || token.indexOf('E') >= 0) {
                return Double.parseDouble(token);
            }
            return Long.parseLong(token);
        }

        private String readString() {
            expect('"');
            StringBuilder out = new StringBuilder();
            while (index < input.length()) {
                char c = input.charAt(index++);
                if (c == '"') {
                    return out.toString();
                }
                if (c == '\\') {
                    if (index >= input.length()) {
                        throw new IllegalArgumentException("unterminated string escape");
                    }
                    char esc = input.charAt(index++);
                    switch (esc) {
                        case '"':
                        case '\\':
                        case '/':
                            out.append(esc);
                            break;
                        case 'b':
                            out.append('\b');
                            break;
                        case 'f':
                            out.append('\f');
                            break;
                        case 'n':
                            out.append('\n');
                            break;
                        case 'r':
                            out.append('\r');
                            break;
                        case 't':
                            out.append('\t');
                            break;
                        case 'u':
                            if (index + 3 >= input.length()) {
                                throw new IllegalArgumentException("invalid unicode escape");
                            }
                            String hex = input.substring(index, index + 4);
                            out.append((char) Integer.parseInt(hex, 16));
                            index += 4;
                            break;
                        default:
                            throw new IllegalArgumentException("invalid string escape");
                    }
                    continue;
                }
                out.append(c);
            }
            throw new IllegalArgumentException("unterminated string");
        }

        private char peek() {
            if (index >= input.length()) {
                throw new IllegalArgumentException("unexpected end");
            }
            return input.charAt(index);
        }

        private void expect(char expected) {
            skipWhitespace();
            if (index >= input.length() || input.charAt(index) != expected) {
                throw new IllegalArgumentException("expected " + expected + " at " + index);
            }
            index++;
        }

        private void skipWhitespace() {
            while (index < input.length()) {
                char c = input.charAt(index);
                if (c != ' ' && c != '\n' && c != '\r' && c != '\t') {
                    return;
                }
                index++;
            }
        }
    }

    private static final class Writer {

        String write(Object value) {
            if (value == null) {
                return "null";
            }
            if (value instanceof String s) {
                return quote(s);
            }
            if (value instanceof Boolean || value instanceof Integer || value instanceof Long || value instanceof Double
                    || value instanceof Float || value instanceof Short || value instanceof Byte) {
                return value.toString();
            }
            if (value instanceof Map<?, ?> map) {
                return writeMap(map);
            }
            if (value instanceof List<?> list) {
                return writeList(list);
            }
            throw new IllegalArgumentException("unsupported json value");
        }

        private String writeMap(Map<?, ?> map) {
            StringBuilder out = new StringBuilder();
            out.append('{');
            TreeMap<String, Object> sorted = new TreeMap<>();
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                sorted.put(String.valueOf(entry.getKey()), entry.getValue());
            }

            boolean first = true;
            for (Map.Entry<String, Object> entry : sorted.entrySet()) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                out.append(quote(entry.getKey()));
                out.append(':');
                out.append(write(entry.getValue()));
            }
            out.append('}');
            return out.toString();
        }

        private String writeList(List<?> list) {
            StringBuilder out = new StringBuilder();
            out.append('[');
            boolean first = true;
            for (Object item : list) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                out.append(write(item));
            }
            out.append(']');
            return out.toString();
        }

        private String quote(String value) {
            StringBuilder out = new StringBuilder();
            out.append('"');
            for (int i = 0; i < value.length(); i++) {
                char c = value.charAt(i);
                if (c == '"') {
                    out.append("\\\"");
                    continue;
                }
                if (c == '\\') {
                    out.append("\\\\");
                    continue;
                }
                if (c == '\b') {
                    out.append("\\b");
                    continue;
                }
                if (c == '\f') {
                    out.append("\\f");
                    continue;
                }
                if (c == '\n') {
                    out.append("\\n");
                    continue;
                }
                if (c == '\r') {
                    out.append("\\r");
                    continue;
                }
                if (c == '\t') {
                    out.append("\\t");
                    continue;
                }
                if (c < 0x20) {
                    out.append(String.format("\\u%04x", (int) c));
                    continue;
                }
                out.append(c);
            }
            out.append('"');
            return out.toString();
        }
    }
}
