package gitignore.matcher;

final class PathValidator {
    private PathValidator() {}

    static void validateBasePath(String path) {
        if (path == null) {
            throw new IgnoreMatcherException("invalid_path", "path is null");
        }
        if (!path.isEmpty()) {
            validateRelativePath(path);
        }
    }

    static void validateEntryPath(String path) {
        if (path == null || path.isEmpty()) {
            throw new IgnoreMatcherException("invalid_path", "entry path is empty");
        }
        validateRelativePath(path);
    }

    private static void validateRelativePath(String path) {
        if (path.startsWith("/") || path.endsWith("/") || path.indexOf('\\') >= 0 || path.indexOf('\0') >= 0) {
            throw new IgnoreMatcherException("invalid_path", "invalid relative path");
        }
        String[] segments = path.split("/", -1);
        for (String segment : segments) {
            if (segment.isEmpty() || segment.equals(".") || segment.equals("..")) {
                throw new IgnoreMatcherException("invalid_path", "invalid path segment");
            }
        }
    }
}
