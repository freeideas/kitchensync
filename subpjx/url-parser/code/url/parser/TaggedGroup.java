package url.parser;

import java.util.List;

public record TaggedGroup(Role role, List<ParsedUrl> urls) {}
