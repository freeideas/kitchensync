package kitchensync;

import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.Semaphore;

final class SftpPoolTrace {
    private final Logger logger;
    private final Map<String, Integer> activeByEndpoint = new HashMap<>();
    private final Map<String, Semaphore> permitsByEndpoint = new HashMap<>();

    SftpPoolTrace(Logger logger) {
        this.logger = logger;
    }

    void acquire(String endpoint, int maximum) throws InterruptedException {
        semaphore(endpoint, maximum).acquire();
        synchronized (this) {
            int active = activeByEndpoint.getOrDefault(endpoint, 0) + 1;
            activeByEndpoint.put(endpoint, active);
            log(endpoint, active, maximum);
        }
    }

    void release(String endpoint, int maximum) {
        synchronized (this) {
            int active = Math.max(0, activeByEndpoint.getOrDefault(endpoint, 0) - 1);
            activeByEndpoint.put(endpoint, active);
            log(endpoint, active, maximum);
        }
        semaphore(endpoint, maximum).release();
    }

    private synchronized Semaphore semaphore(String endpoint, int maximum) {
        return permitsByEndpoint.computeIfAbsent(endpoint, ignored -> new Semaphore(maximum));
    }

    private void log(String endpoint, int active, int maximum) {
        logger.trace("endpoint=" + endpoint + " connections=" + active + "/" + maximum);
    }
}
