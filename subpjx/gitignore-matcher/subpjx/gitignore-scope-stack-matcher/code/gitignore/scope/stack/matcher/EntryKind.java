package gitignore.scope.stack.matcher;

public enum EntryKind {
    FILE, DIR, SYMLINK, SPECIAL;

    public static EntryKind fromString(String s) {
        if (s == null) throw new IllegalArgumentException("kind is null");
        switch (s) {
            case "file":    return FILE;
            case "dir":     return DIR;
            case "symlink": return SYMLINK;
            case "special": return SPECIAL;
            default: throw new IllegalArgumentException("unknown kind: " + s);
        }
    }
}
