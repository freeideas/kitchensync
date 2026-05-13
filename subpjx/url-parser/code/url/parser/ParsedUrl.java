package url.parser;

import java.util.Map;

public record ParsedUrl(
    String scheme,
    String user,
    String password,
    String host,
    Integer port,
    String path,
    Map<String, String> query,
    String identity
) {}
