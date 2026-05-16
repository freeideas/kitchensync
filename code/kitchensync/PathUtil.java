package kitchensync;

final class PathUtil {
    private PathUtil() {
    }

    static String child(String parent, String name) {
        return parent == null || parent.isEmpty() ? name : parent + "/" + name;
    }

    static String parent(String path) {
        int slash = path.lastIndexOf('/');
        return slash < 0 ? "" : path.substring(0, slash);
    }

    static String basename(String path) {
        int slash = path.lastIndexOf('/');
        return slash < 0 ? path : path.substring(slash + 1);
    }
}
