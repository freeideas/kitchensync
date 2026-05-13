package gitignore.matcher;

import java.util.List;

public final class Patterns {
    private final List<ParsedPattern> list;

    public Patterns(List<ParsedPattern> list) {
        this.list = list;
    }

    List<ParsedPattern> list() {
        return list;
    }
}
