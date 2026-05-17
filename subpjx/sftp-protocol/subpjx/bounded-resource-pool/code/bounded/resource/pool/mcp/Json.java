package bounded.resource.pool.mcp;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

final class Json {
    private Json() {
    }

    static Object parse(String text) {
        Parser parser = new Parser(text);
        Object value = parser.value();
        parser.skipWhitespace();
        if (!parser.done()) {
            throw new IllegalArgumentException("trailing content");
        }
        return value;
    }

    static String write(Object value) {
        StringBuilder builder = new StringBuilder();
        writeValue(builder, value);
        return builder.toString();
    }

    private static void writeValue(StringBuilder builder, Object value) {
        if (value == null) {
            builder.append("null");
        } else if (value instanceof String string) {
            writeString(builder, string);
        } else if (value instanceof Number || value instanceof Boolean) {
            builder.append(value);
        } else if (value instanceof Map<?, ?> map) {
            builder.append('{');
            boolean first = true;
            Map<String, Object> sorted = new TreeMap<>();
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                sorted.put((String) entry.getKey(), entry.getValue());
            }
            for (Map.Entry<String, Object> entry : sorted.entrySet()) {
                if (!first) {
                    builder.append(',');
                }
                first = false;
                writeString(builder, entry.getKey());
                builder.append(':');
                writeValue(builder, entry.getValue());
            }
            builder.append('}');
        } else if (value instanceof Iterable<?> iterable) {
            builder.append('[');
            boolean first = true;
            for (Object item : iterable) {
                if (!first) {
                    builder.append(',');
                }
                first = false;
                writeValue(builder, item);
            }
            builder.append(']');
        } else {
            throw new IllegalArgumentException("unsupported json value");
        }
    }

    private static void writeString(StringBuilder builder, String value) {
        builder.append('"');
        for (int index = 0; index < value.length(); index++) {
            char ch = value.charAt(index);
            switch (ch) {
                case '"' -> builder.append("\\\"");
                case '\\' -> builder.append("\\\\");
                case '\b' -> builder.append("\\b");
                case '\f' -> builder.append("\\f");
                case '\n' -> builder.append("\\n");
                case '\r' -> builder.append("\\r");
                case '\t' -> builder.append("\\t");
                default -> {
                    if (ch < 0x20) {
                        builder.append(String.format("\\u%04x", (int) ch));
                    } else {
                        builder.append(ch);
                    }
                }
            }
        }
        builder.append('"');
    }

    private static final class Parser {
        private final String text;
        private int position;

        private Parser(String text) {
            this.text = text;
        }

        private Object value() {
            skipWhitespace();
            if (done()) {
                throw new IllegalArgumentException("missing value");
            }
            char ch = text.charAt(position);
            return switch (ch) {
                case '{' -> object();
                case '[' -> array();
                case '"' -> string();
                case 't' -> literal("true", Boolean.TRUE);
                case 'f' -> literal("false", Boolean.FALSE);
                case 'n' -> literal("null", null);
                default -> number();
            };
        }

        private Map<String, Object> object() {
            position++;
            Map<String, Object> object = new TreeMap<>();
            skipWhitespace();
            if (consume('}')) {
                return object;
            }
            while (true) {
                skipWhitespace();
                if (done() || text.charAt(position) != '"') {
                    throw new IllegalArgumentException("object key must be string");
                }
                String key = string();
                skipWhitespace();
                expect(':');
                object.put(key, value());
                skipWhitespace();
                if (consume('}')) {
                    return object;
                }
                expect(',');
            }
        }

        private List<Object> array() {
            position++;
            List<Object> array = new ArrayList<>();
            skipWhitespace();
            if (consume(']')) {
                return array;
            }
            while (true) {
                array.add(value());
                skipWhitespace();
                if (consume(']')) {
                    return array;
                }
                expect(',');
            }
        }

        private String string() {
            expect('"');
            StringBuilder builder = new StringBuilder();
            while (!done()) {
                char ch = text.charAt(position++);
                if (ch == '"') {
                    return builder.toString();
                }
                if (ch != '\\') {
                    builder.append(ch);
                    continue;
                }
                if (done()) {
                    throw new IllegalArgumentException("bad escape");
                }
                char escaped = text.charAt(position++);
                switch (escaped) {
                    case '"', '\\', '/' -> builder.append(escaped);
                    case 'b' -> builder.append('\b');
                    case 'f' -> builder.append('\f');
                    case 'n' -> builder.append('\n');
                    case 'r' -> builder.append('\r');
                    case 't' -> builder.append('\t');
                    case 'u' -> {
                        if (position + 4 > text.length()) {
                            throw new IllegalArgumentException("bad unicode escape");
                        }
                        builder.append((char) Integer.parseInt(text.substring(position, position + 4), 16));
                        position += 4;
                    }
                    default -> throw new IllegalArgumentException("bad escape");
                }
            }
            throw new IllegalArgumentException("unterminated string");
        }

        private Object number() {
            int start = position;
            if (consume('-')) {
                if (done()) {
                    throw new IllegalArgumentException("bad number");
                }
            }
            while (!done() && Character.isDigit(text.charAt(position))) {
                position++;
            }
            if (!done() && text.charAt(position) == '.') {
                position++;
                while (!done() && Character.isDigit(text.charAt(position))) {
                    position++;
                }
                return Double.parseDouble(text.substring(start, position));
            }
            if (!done() && (text.charAt(position) == 'e' || text.charAt(position) == 'E')) {
                position++;
                if (!done() && (text.charAt(position) == '-' || text.charAt(position) == '+')) {
                    position++;
                }
                while (!done() && Character.isDigit(text.charAt(position))) {
                    position++;
                }
                return Double.parseDouble(text.substring(start, position));
            }
            if (start == position || (start + 1 == position && text.charAt(start) == '-')) {
                throw new IllegalArgumentException("bad number");
            }
            return Long.parseLong(text.substring(start, position));
        }

        private Object literal(String literal, Object value) {
            if (!text.startsWith(literal, position)) {
                throw new IllegalArgumentException("bad literal");
            }
            position += literal.length();
            return value;
        }

        private boolean consume(char expected) {
            if (!done() && text.charAt(position) == expected) {
                position++;
                return true;
            }
            return false;
        }

        private void expect(char expected) {
            if (!consume(expected)) {
                throw new IllegalArgumentException("expected " + expected);
            }
        }

        private void skipWhitespace() {
            while (!done() && Character.isWhitespace(text.charAt(position))) {
                position++;
            }
        }

        private boolean done() {
            return position >= text.length();
        }
    }
}
