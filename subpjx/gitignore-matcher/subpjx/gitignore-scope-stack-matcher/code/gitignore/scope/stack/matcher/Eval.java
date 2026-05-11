package gitignore.scope.stack.matcher;

/** Evaluates `is_ignored` for a Matcher against a candidate path. */
public final class Eval {
    private Eval() {}

    public static boolean isIgnored(Matcher m, String path, boolean isDir) {
        boolean userIgnored = false;
        boolean userNegationApplied = false;

        for (Layer layer : m.layers()) {
            String suffix = suffixBelow(path, layer.scopeDir());
            if (suffix == null) continue;
            for (CompiledPattern p : layer.patternSet()) {
                if (p.isDirOnly() && !isDir) continue;
                if (!applies(p, suffix)) continue;
                if (p.isNegation()) {
                    userIgnored = false;
                    userNegationApplied = true;
                } else {
                    userIgnored = true;
                    userNegationApplied = false;
                }
            }
        }

        if (hasKitchensyncSegment(path)) return true;
        if (userIgnored) return true;
        if (hasGitFirstSegment(path) && !userNegationApplied) return true;
        return false;
    }

    public static boolean isIgnoredEntry(Matcher m, String path, EntryKind kind) {
        switch (kind) {
            case FILE:    return isIgnored(m, path, false);
            case DIR:     return isIgnored(m, path, true);
            case SYMLINK: return true;
            case SPECIAL: return true;
            default: throw new IllegalArgumentException("unknown kind: " + kind);
        }
    }

    /** Portion of `path` below `scopeDir`, or null if `path` is not within `scopeDir`. */
    static String suffixBelow(String path, String scopeDir) {
        if (scopeDir.isEmpty()) return path;
        if (path.equals(scopeDir)) return "";
        String prefix = scopeDir + "/";
        if (path.startsWith(prefix)) return path.substring(prefix.length());
        return null;
    }

    private static boolean applies(CompiledPattern p, String suffix) {
        if (p.isAnchored()) {
            return Glob.matches(p.body(), suffix);
        }
        if (p.body().indexOf('/') >= 0) {
            return Glob.matches(p.body(), suffix);
        }
        if (suffix.isEmpty()) return false;
        for (String seg : suffix.split("/")) {
            if (Glob.matches(p.body(), seg)) return true;
        }
        return false;
    }

    private static boolean hasKitchensyncSegment(String path) {
        if (path.isEmpty()) return false;
        for (String seg : path.split("/")) {
            if (seg.equals(".kitchensync")) return true;
        }
        return false;
    }

    private static boolean hasGitFirstSegment(String path) {
        if (path.equals(".git")) return true;
        return path.startsWith(".git/");
    }
}
