package kitchensync;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;

import sftp.protocol.SftpPoolRegistry;

final class PeerConnector {
    private final RunOptions options;
    private final SftpPoolRegistry pools;
    private final Logger logger;
    private final SftpPoolTrace poolTrace;

    PeerConnector(RunOptions options, SftpPoolRegistry pools, Logger logger, SftpPoolTrace poolTrace) {
        this.options = options;
        this.pools = pools;
        this.logger = logger;
        this.poolTrace = poolTrace;
    }

    List<ConnectedPeer> connectAll(ExecutorService executor) {
        List<CompletableFuture<ConnectedPeer>> futures = new ArrayList<>();
        for (PeerArgument argument : options.peers) {
            futures.add(CompletableFuture.supplyAsync(() -> connect(argument), executor));
        }
        List<ConnectedPeer> connected = new ArrayList<>();
        for (CompletableFuture<ConnectedPeer> future : futures) {
            ConnectedPeer peer = future.join();
            if (peer != null) {
                connected.add(peer);
            }
        }
        Map<String, ConnectedPeer> unique = new LinkedHashMap<>();
        for (ConnectedPeer peer : connected) {
            unique.putIfAbsent(peer.url().normalized(), peer);
        }
        return applyEndpointPoolSettings(new ArrayList<>(unique.values()));
    }

    private static List<ConnectedPeer> applyEndpointPoolSettings(List<ConnectedPeer> connected) {
        Map<String, SftpTransport> firstByEndpoint = new LinkedHashMap<>();
        List<ConnectedPeer> adjusted = new ArrayList<>();
        for (ConnectedPeer peer : connected) {
            if (peer.transport() instanceof SftpTransport sftp) {
                SftpTransport first = firstByEndpoint.putIfAbsent(sftp.endpointKey(), sftp);
                if (first != null) {
                    adjusted.add(new ConnectedPeer(peer.argument(), peer.url(), sftp.withPoolSettingsFrom(first)));
                    continue;
                }
            }
            adjusted.add(peer);
        }
        return adjusted;
    }

    private ConnectedPeer connect(PeerArgument argument) {
        for (String raw : argument.urls()) {
            try {
                PeerUrl url = UrlParser.parse(raw, options);
                Transport transport;
                if (url.scheme().equals("file")) {
                    transport = LocalTransport.connect(url.localPath().orElseThrow());
                } else {
                    transport = SftpTransport.connect(url.sftp().orElseThrow(), url.config(), pools, poolTrace);
                }
                return new ConnectedPeer(argument, url, transport);
            } catch (RuntimeException | TransportException ex) {
                logger.error("unreachable peer URL: " + raw);
            }
        }
        logger.error("unreachable peer: " + String.join(",", argument.urls()));
        return null;
    }
}
