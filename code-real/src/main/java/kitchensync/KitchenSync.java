package kitchensync;

import cli.parser.CliParser;
import cli.parser.GlobalOptions;
import cli.parser.ParseOutcome;
import cli.parser.PeerSpec;
import cli.parser.Prefix;
import cli.parser.SyncConfig;
import cli.parser.UrlSpec;
import cli.parser.Verbosity;
import connection.pool.AllUrlsFailedException;
import connection.pool.ConnectionPool;
import connection.pool.Pool;
import peer.fs.types.PeerFs;
import snapshot.db.OpenResult;
import snapshot.db.SnapshotDb;
import snapshot.db.SnapshotDbService;
import sync.engine.SyncEngine;
import sync.engine.SyncOptions;

import java.io.IOException;
import java.net.URI;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;

public class KitchenSync {

    private record PeerConn(
        connection.pool.ActivePeer rawConn,
        Pool transferPool,
        boolean isCanon,
        boolean isSubordinate,
        String peerRoot
    ) {
        String peerUrl()     { return rawConn.peerUrl(); }
        PeerFs listingConn() { return rawConn.listingConn(); }
    }

    // @formatter:off
    private static final String HELP_TEXT =
"Usage: java -jar kitchensync.jar [options] <peer> <peer> [<peer>...]\n" +
"\n" +
"Synchronize file trees across multiple peers.\n" +
"\n" +
"Running with no arguments prints this help. See README.md for full docs.\n" +
"\n" +
"Peers:\n" +
"  /path or c:\\path                 Local path (same as file://)\n" +
"  sftp://user@host/path            Remote over SSH\n" +
"  sftp://user@host:port/path       Non-standard SSH port\n" +
"  sftp://user:password@host/path   Inline password (prefer SSH keys)\n" +
"\n" +
"Prefix modifiers:\n" +
"  +<peer>                          Canon — this peer's state wins all conflicts\n" +
"  -<peer>                          Subordinate — overwritten to match the group\n" +
"\n" +
"Fallback URLs (multiple paths to the same data):\n" +
"  [url1,url2,...]                  Try in order, first that connects wins\n" +
"  +[url1,url2,...]                 Canon peer with fallbacks\n" +
"  -[url1,url2,...]                 Subordinate peer with fallbacks\n" +
"\n" +
"Per-URL settings (query string, inside quotes):\n" +
"  \"sftp://host/path?mc=5\"          Max connections for this URL\n" +
"  \"sftp://host/path?ct=60\"         Connection timeout for this URL\n" +
"  \"sftp://host/path?mc=5&ct=60\"    Both\n" +
"\n" +
"Options:\n" +
"  -h, --help, /?                      Show this help\n" +
"  --mc N             Max concurrent connections per URL (default: 10)\n" +
"  --ct N             SSH handshake timeout in seconds (default: 30)\n" +
"  -vl LEVEL          Verbosity level: error, info, debug, trace (default: info)\n" +
"  --xd N             Delete stale TMP staging after N days (default: 2)\n" +
"  --bd N             Delete displaced files (BAK/) after N days (default: 90)\n" +
"  --td N             Forget deletion records after N days (default: 180)\n" +
"\n" +
"Quick start:\n" +
"  java -jar kitchensync.jar +c:/photos sftp://user@host/photos      First sync (c: is canon)\n" +
"  java -jar kitchensync.jar c:/photos sftp://host/photos            Bidirectional\n" +
"  java -jar kitchensync.jar c:/photos sftp://host/photos -/mnt/usb  Add USB as subordinate\n" +
"  java -jar kitchensync.jar c:/photos \"sftp://user:p%40ss@host/photos\"  Inline password\n" +
"\n" +
"Canon (+) is required on first sync when no peer has snapshot history.\n" +
"After the first sync, bidirectional sync works without canon.\n" +
"\n" +
"Tip: if ssh user@host and cd /path works, sftp://user@host/path will too.\n" +
"\n" +
"Displaced files are recoverable from .kitchensync/BAK/ (kept for --bd days).\n";
    // @formatter:on

    public static int run(String[] args) {
        ParseOutcome outcome = CliParser.parse(args);
        return switch (outcome) {
            case ParseOutcome.HelpOnly() -> {
                System.out.print(HELP_TEXT);
                yield 0;
            }
            case ParseOutcome.Error(var msg) -> {
                System.out.println(msg);
                System.out.print(HELP_TEXT);
                yield 1;
            }
            case ParseOutcome.Config(var config) -> doSync(config);
        };
    }

    private static int doSync(SyncConfig config) {
        GlobalOptions opts = config.options();
        ConnectionPool connPool = new ConnectionPool();
        SnapshotDbService snapshotSvc = new SnapshotDbService();
        SyncEngine engine = new SyncEngine();

        boolean hasCanonSpec = config.peers().stream().anyMatch(p -> p.prefix() == Prefix.CANON);

        // Connect all peers in parallel; skip unreachable ones
        List<PeerConn> reachable = connectAllParallel(config.peers(), opts, connPool);

        if (reachable.size() < 2) {
            System.out.println("Fewer than two peers are reachable");
            return 1;
        }
        if (hasCanonSpec && reachable.stream().noneMatch(PeerConn::isCanon)) {
            System.out.println("Canon peer is unreachable");
            return 1;
        }

        // Local temp root for snapshot downloads
        Path tmpRoot;
        try {
            tmpRoot = Files.createTempDirectory("kitchensync-");
        } catch (IOException e) {
            System.out.println("Cannot create temp directory: " + e.getMessage());
            return 1;
        }

        // Download (or create) each peer's snapshot
        record PeerState(PeerConn conn, SnapshotDb db, boolean hadSnapshot, boolean isSubordinate) {}

        List<PeerState> states = new ArrayList<>();
        for (PeerConn pc : reachable) {
            Path localTmp = tmpRoot.resolve(UUID.randomUUID().toString());
            try {
                Files.createDirectories(localTmp);
                OpenResult or = snapshotSvc.open(pc.listingConn(), pc.peerRoot(), localTmp);
                boolean autoSub = !or.hadSnapshot() && !pc.isCanon();
                states.add(new PeerState(pc, or.db(), or.hadSnapshot(), pc.isSubordinate() || autoSub));
            } catch (IOException e) {
                System.out.println("Error opening snapshot for " + pc.peerUrl() + ": " + e.getMessage());
            }
        }

        if (states.size() < 2) {
            System.out.println("Fewer than two peers are reachable");
            return 1;
        }

        boolean anyHadSnapshot = states.stream().anyMatch(PeerState::hadSnapshot);
        if (!anyHadSnapshot && !hasCanonSpec) {
            System.out.println("First sync? Mark the authoritative peer with a leading +");
            return 1;
        }

        boolean anyContributing = states.stream().anyMatch(s -> !s.isSubordinate());
        if (!anyContributing) {
            System.out.println("No contributing peer reachable — cannot make sync decisions");
            return 1;
        }

        String syncTimestamp = timestamp();

        // Purge stale snapshot rows before traversal
        for (PeerState s : states) {
            snapshotSvc.purgeOld(s.db(), opts.tombstoneRetentionDays(), syncTimestamp);
        }

        // Build sync engine peer list
        List<sync.engine.ActivePeer> enginePeers = states.stream()
            .map(s -> new sync.engine.ActivePeer(
                s.conn().peerUrl(),
                s.conn().listingConn(),
                s.conn().transferPool(),
                s.conn().isCanon(),
                s.isSubordinate()))
            .toList();

        Map<String, SnapshotDb> snapshots = new LinkedHashMap<>();
        for (PeerState s : states) {
            snapshots.put(s.conn().peerUrl(), s.db());
        }

        SyncOptions syncOpts = new SyncOptions(
            toEngineVerbosity(opts.verbosity()),
            opts.expireXDays(),
            opts.bakRetentionDays());

        engine.run(enginePeers, snapshots, syncOpts);

        // Upload updated snapshots back to peers
        for (PeerState s : states) {
            try {
                snapshotSvc.upload(s.db(), s.conn().listingConn(), s.conn().peerRoot(), syncTimestamp);
            } catch (IOException e) {
                System.out.println("Error: snapshot upload failed for " + s.conn().peerUrl() + ": " + e.getMessage());
            }
            snapshotSvc.close(s.db());
        }

        connPool.disconnectAll(
            states.stream().map(s -> s.conn().rawConn()).toList(),
            states.stream().map(s -> s.conn().transferPool()).toList());

        return 0;
    }

    private static List<PeerConn> connectAllParallel(List<PeerSpec> specs, GlobalOptions opts, ConnectionPool connPool) {
        List<CompletableFuture<PeerConn>> futures = specs.stream()
            .map(spec -> CompletableFuture.supplyAsync(() -> connectOne(spec, opts, connPool)))
            .toList();
        return futures.stream()
            .map(CompletableFuture::join)
            .filter(Objects::nonNull)
            .toList();
    }

    private static PeerConn connectOne(PeerSpec spec, GlobalOptions opts, ConnectionPool connPool) {
        List<connection.pool.UrlSpec> urlSpecs = spec.fallbackUrls().stream()
            .map(u -> new connection.pool.UrlSpec(u.rawUrl(), u.maxConnections(), u.timeoutSeconds()))
            .toList();
        try {
            connection.pool.ActivePeer conn = connPool.connect(urlSpecs, opts.timeoutSeconds());
            String activeUrl = conn.peerUrl();

            int mc = opts.maxConnections();
            for (UrlSpec us : spec.fallbackUrls()) {
                if (SnapshotDbService.normalizeUrl(us.rawUrl()).equals(activeUrl) && us.maxConnections() != null) {
                    mc = us.maxConnections();
                    break;
                }
            }

            Pool transferPool = connPool.createPool(activeUrl, mc, opts.timeoutSeconds());
            String peerRoot = URI.create(activeUrl).getPath();

            return new PeerConn(conn, transferPool,
                spec.prefix() == Prefix.CANON,
                spec.prefix() == Prefix.SUBORDINATE,
                peerRoot);
        } catch (AllUrlsFailedException e) {
            System.out.println("Warning: " + e.getMessage());
            return null;
        }
    }

    private static sync.engine.Verbosity toEngineVerbosity(Verbosity v) {
        return switch (v) {
            case ERROR -> sync.engine.Verbosity.ERROR;
            case INFO  -> sync.engine.Verbosity.INFO;
            case DEBUG -> sync.engine.Verbosity.DEBUG;
            case TRACE -> sync.engine.Verbosity.TRACE;
        };
    }

    private static String timestamp() {
        ZonedDateTime now = ZonedDateTime.now(ZoneOffset.UTC);
        return String.format("%04d-%02d-%02d_%02d-%02d-%02d_%06dZ",
            now.getYear(), now.getMonthValue(), now.getDayOfMonth(),
            now.getHour(), now.getMinute(), now.getSecond(),
            now.getNano() / 1000);
    }
}
