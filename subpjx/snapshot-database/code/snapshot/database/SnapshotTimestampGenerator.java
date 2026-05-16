package snapshot.database;

import java.time.Clock;
import java.time.Instant;

public final class SnapshotTimestampGenerator {
    private final Clock clock;
    private long lastMicros = Long.MIN_VALUE;

    public SnapshotTimestampGenerator() {
        this(Clock.systemUTC());
    }

    public SnapshotTimestampGenerator(Clock clock) {
        if (clock == null) {
            throw new SnapshotDatabaseException("invalid_timestamp", "clock is required");
        }
        this.clock = clock;
    }

    public synchronized SnapshotTime next() {
        Instant now = clock.instant();
        long micros = Math.addExact(Math.multiplyExact(now.getEpochSecond(), 1_000_000L), now.getNano() / 1_000L);
        if (micros <= lastMicros) {
            micros = Math.addExact(lastMicros, 1L);
        }
        lastMicros = micros;
        return SnapshotTime.fromEpochMicros(micros);
    }
}
