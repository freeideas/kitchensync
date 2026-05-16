package gitignore.pattern.set.mcp;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

final class Json {
    private Json() {
    }

    static Object parse(String text) {
        Parser parser = new Parser(text);
        Object value = parser.parseValue();
        parser.skipWhitespace();
        if (!parser.atEnd()) {
            throw new IllegalArgumentException("trailing JSON content");
        }
        return value;
    }

    static String write(Object value) {
        StringBuilder out = new StringBuilder();
        writeValue(out, value);
        return out.toString();
    }

    private static void writeValue(StringBuilder out, Object value) {
        if (value == null) {
            out.append("null");
        } else if (value instanceof String string) {
            writeString(out, string);
        } else if (value instanceof Boolean bool) {
            out.append(bool);
        } else if (value instanceof Number number) {
            out.append(number);
        } else if (value instanceof Map<?, ?> map) {
            out.append('{');
            List<Map.Entry<?, ?>> entries = new ArrayList<>(map.entrySet());
            entries.sort(Comparator.comparing(entry -> String.valueOf(entry.getKey())));
            for (int i = 0; i < entries.size(); i++) {
                if (i > 0) {
                    out.append(',');
                }
                writeString(out, String.valueOf(entries.get(i).getKey()));
                out.append(':');
                writeValue(out, entries.get(i).getValue());
            }
            out.append('}');
        } else if (value instanceof Iterable<?> iterable) {
            out.append('[');
            boolean first = true;
            for (Object item : iterable) {
                if (!first) {
                    out.append(',');
                }
                writeValue(out, item);
                first = false;
            }
            out.append(']');
        } else {
            writeString(out, String.valueOf(value));
        }
    }

    private static void writeString(StringBuilder out, String value) {
        out.append('"');
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            if (ch == '"' || ch == '\\') {
                out.append('\\').append(ch);
            } else if (ch == '\b') {
                out.append("\\b");
            } else if (ch == '\f') {
                out.append("\\f");
            } else if (ch == '\n') {
                out.append("\\n");
            } else if (ch == '\r') {
                out.append("\\r");
            } else if (ch == '\t') {
                out.append("\\t");
            } else if (ch < 0x20) {
                out.append(String.format("\\u%04x", (int) ch));
            } else {
                out.append(ch);
            }
        }
        out.append('"');
    }

    private static final class Parser {
        private final String text;
        private int index;

        private Parser(String text) {
            this.text = text;
        }

        private Object parseValue() {
            skipWhitespace();
            if (atEnd()) {
                throw new IllegalArgumentException("missing JSON value");
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
            if (text.startsWith("true", index)) {
                index += 4;
                return Boolean.TRUE;
            }
            if (text.startsWith("false", index)) {
                index += 5;
                return Boolean.FALSE;
            }
            if (text.startsWith("null", index)) {
                index += 4;
                return null;
            }
            if (ch == '-' || Character.isDigit(ch)) {
                return parseNumber();
            }
            throw new IllegalArgumentException("invalid JSON value");
        }

        private Map<String, Object> parseObject() {
            index++;
            Map<String, Object> object = new LinkedHashMap<>();
            skipWhitespace();
            if (consume('}')) {
                return object;
            }
            while (true) {
                skipWhitespace();
                if (atEnd() || text.charAt(index) != '"') {
                    throw new IllegalArgumentException("object key must be a string");
                }
                String key = parseString();
                skipWhitespace();
                require(':');
                object.put(key, parseValue());
                skipWhitespace();
                if (consume('}')) {
                    return object;
                }
                require(',');
            }
        }

        private List<Object> parseArray() {
            index++;
            List<Object> array = new ArrayList<>();
            skipWhitespace();
            if (consume(']')) {
                return array;
            }
            while (true) {
                array.add(parseValue());
                skipWhitespace();
                if (consume(']')) {
                    return array;
                }
                require(',');
            }
        }

        private String parseString() {
            require('"');
            StringBuilder out = new StringBuilder();
            while (!atEnd()) {
                char ch = text.charAt(index++);
                if (ch == '"') {
                    return out.toString();
                }
                if (ch == '\\') {
                    if (atEnd()) {
                        throw new IllegalArgumentException("unterminated escape");
                    }
                    char escaped = text.charAt(index++);
                    if (escaped == '"' || escaped == '\\' || escaped == '/') {
                        out.append(escaped);
                    } else if (escaped == 'b') {
                        out.append('\b');
                    } else if (escaped == 'f') {
                        out.append('\f');
                    } else if (escaped == 'n') {
                        out.append('\n');
                    } else if (escaped == 'r') {
                        out.append('\r');
                    } else if (escaped == 't') {
                        out.append('\t');
                    } else if (escaped == 'u') {
                        out.append(parseUnicode());
                    } else {
                        throw new IllegalArgumentException("invalid escape");
                    }
                } else {
                    if (ch < 0x20) {
                        throw new IllegalArgumentException("control character in string");
                    }
                    out.append(ch);
                }
            }
            throw new IllegalArgumentException("unterminated string");
        }

        private char parseUnicode() {
            if (index + 4 > text.length()) {
                throw new IllegalArgumentException("short unicode escape");
            }
            int value = 0;
            for (int i = 0; i < 4; i++) {
                int digit = Character.digit(text.charAt(index++), 16);
                if (digit < 0) {
                    throw new IllegalArgumentException("invalid unicode escape");
                }
                value = value * 16 + digit;
            }
            return (char) value;
        }

        private Number parseNumber() {
            int start = index;
            if (consume('-') && atEnd()) {
                throw new IllegalArgumentException("invalid number");
            }
            while (!atEnd() && Character.isDigit(text.charAt(index))) {
                index++;
            }
            boolean floating = false;
            if (!atEnd() && text.charAt(index) == '.') {
                floating = true;
                index++;
                while (!atEnd() && Character.isDigit(text.charAt(index))) {
                    index++;
                }
            }
            if (!atEnd() && (text.charAt(index) == 'e' || text.charAt(index) == 'E')) {
                floating = true;
                index++;
                if (!atEnd() && (text.charAt(index) == '+' || text.charAt(index) == '-')) {
                    index++;
                }
                while (!atEnd() && Character.isDigit(text.charAt(index))) {
                    index++;
                }
            }
            String token = text.substring(start, index);
            if (floating) {
                return Double.valueOf(token);
            }
            return Long.valueOf(token);
        }

        private void skipWhitespace() {
            while (!atEnd()) {
                char ch = text.charAt(index);
                if (ch != ' ' && ch != '\t' && ch != '\n' && ch != '\r') {
                    return;
                }
                index++;
            }
        }

        private boolean consume(char expected) {
            if (!atEnd() && text.charAt(index) == expected) {
                index++;
                return true;
            }
            return false;
        }

        private void require(char expected) {
            if (!consume(expected)) {
                throw new IllegalArgumentException("expected " + expected);
            }
        }

        private boolean atEnd() {
            return index >= text.length();
        }
    }
}
