package kitchensync;

import java.util.List;

final class PathExcludes {
    private PathExcludes() {
    }

    static String validate(String text) {
        if (text == null || text.isEmpty()) {
            throw new CliParser.ValidationException("Invalid value for -x: " + text);
        }
        if (text.indexOf('\0') >= 0 || text.indexOf('\\') >= 0 || text.startsWith("/") || text.endsWith("/")) {
            throw new CliParser.ValidationException("Invalid value for -x: " + text);
        }
        String[] parts = text.split("/", -1);
        for (String part : parts) {
            if (part.isEmpty() || part.equals(".") || part.equals("..")) {
                throw new CliParser.ValidationException("Invalid value for -x: " + text);
            }
        }
        return text;
    }

    static boolean excluded(List<String> excludes, String path) {
        for (String exclude : excludes) {
            if (path.equals(exclude) || path.startsWith(exclude + "/")) {
                return true;
            }
        }
        return false;
    }
}
