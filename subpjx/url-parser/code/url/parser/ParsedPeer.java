package url.parser;

import java.util.List;

public record ParsedPeer(PeerRole role, List<ParsedUrl> candidates) {
    public ParsedPeer {
        candidates = List.copyOf(candidates);
    }
}
