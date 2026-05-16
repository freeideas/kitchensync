package kitchensync;

import java.time.Instant;
import java.time.ZoneOffset;
import java.time.format.DateTimeFormatter;

import snapshot.database.SnapshotTime;
import snapshot.database.SnapshotTimestampGenerator;

final class TimeUtil {
    private static final DateTimeFormatter FORMATTER = DateTimeFormatter.ofPattern("yyyy-MM-dd_HH-mm-ss_SSSSSS'Z'")
            .withZone(ZoneOffset.UTC);
    private final SnapshotTimestampGenerator generator = new SnapshotTimestampGenerator();

    SnapshotTime nextSnapshotTime() {
        return generator.next();
    }

    String nextText() {
        return nextSnapshotTime().value();
    }

    static SnapshotTime snapshotTime(Instant instant) {
        return SnapshotTime.of(FORMATTER.format(instant));
    }

    static Instant instant(SnapshotTime time) {
        String value = time.value();
        String iso = value.substring(0, 10) + "T" + value.substring(11, 13) + ":" + value.substring(14, 16)
                + ":" + value.substring(17, 19) + "." + value.substring(20, 26) + "Z";
        return Instant.parse(iso);
    }

    static Instant instant(String value) {
        return instant(SnapshotTime.of(value));
    }
}
