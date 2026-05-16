package kitchensync;

import sftp.protocol.PoolEvent;

final class SftpPoolTrace {
    private final Logger logger;

    SftpPoolTrace(Logger logger) {
        this.logger = logger;
    }

    void event(PoolEvent event) {
        logger.trace("endpoint=" + event.endpoint() + " connections=" + event.open_connections() + "/"
                + event.max_connections());
    }
}
