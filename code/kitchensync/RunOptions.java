package kitchensync;

import java.util.ArrayList;
import java.util.List;

final class RunOptions {
    int maxConnections = 10;
    int connectTimeoutSeconds = 30;
    int keepAliveSeconds = 30;
    Verbosity verbosity = Verbosity.info;
    int dirStatusSeconds = 10;
    int tmpRetentionDays = 2;
    int bakRetentionDays = 90;
    int tombstoneRetentionDays = 180;
    final List<String> excludes = new ArrayList<>();
    final List<PeerArgument> peers = new ArrayList<>();
}
