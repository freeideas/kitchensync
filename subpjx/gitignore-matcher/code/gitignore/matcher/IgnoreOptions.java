package gitignore.matcher;

import java.util.LinkedHashSet;
import java.util.Objects;
import java.util.Set;

public final class IgnoreOptions {
    private final Set<String> alwaysExcludedDirectoryNames;
    private final Set<String> defaultExcludedDirectoryNames;
    private final boolean ignoreSymlinks;
    private final boolean ignoreSpecialEntries;

    public IgnoreOptions() {
        this(Set.of(".kitchensync"), Set.of(".git"), true, true);
    }

    public IgnoreOptions(
            Set<String> alwaysExcludedDirectoryNames,
            Set<String> defaultExcludedDirectoryNames,
            boolean ignoreSymlinks,
            boolean ignoreSpecialEntries) {
        this.alwaysExcludedDirectoryNames = copyAndValidate(alwaysExcludedDirectoryNames);
        this.defaultExcludedDirectoryNames = copyAndValidate(defaultExcludedDirectoryNames);
        this.ignoreSymlinks = ignoreSymlinks;
        this.ignoreSpecialEntries = ignoreSpecialEntries;
    }

    public static IgnoreOptions defaults() {
        return new IgnoreOptions();
    }

    public static IgnoreOptions defaultOptions() {
        return defaults();
    }

    public Set<String> alwaysExcludedDirectoryNames() {
        return alwaysExcludedDirectoryNames;
    }

    public Set<String> defaultExcludedDirectoryNames() {
        return defaultExcludedDirectoryNames;
    }

    public boolean ignoreSymlinks() {
        return ignoreSymlinks;
    }

    public boolean ignoreSpecialEntries() {
        return ignoreSpecialEntries;
    }

    private static Set<String> copyAndValidate(Set<String> names) {
        Objects.requireNonNull(names, "names");
        LinkedHashSet<String> copy = new LinkedHashSet<>();
        for (String name : names) {
            validateDirectoryName(name);
            copy.add(name);
        }
        return Set.copyOf(copy);
    }

    static void validateDirectoryName(String name) {
        if (name == null
                || name.isEmpty()
                || name.indexOf('/') >= 0
                || name.indexOf('\\') >= 0
                || name.indexOf('\0') >= 0
                || name.equals(".")
                || name.equals("..")) {
            throw new IgnoreMatcherException("invalid_options", "invalid directory name");
        }
    }
}
