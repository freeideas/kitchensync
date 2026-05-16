package kitchensync;

import java.util.List;

record PeerArgument(PeerModifier modifier, List<String> urls, int index) {
}
