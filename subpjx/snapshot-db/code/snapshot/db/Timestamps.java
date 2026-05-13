package snapshot.db;

import java.time.Instant;
import java.time.LocalDateTime;
import java.time.ZoneOffset;
import java.util.concurrent.atomic.AtomicLong;

public final class Timestamps {

    private static final AtomicLong lastMicros = new AtomicLong(Long.MIN_VALUE);

    private Timestamps() {}

    public static String now() {
        Instant instant = Instant.now();
        long wallMicros = instant.getEpochSecond() * 1_000_000L + instant.getNano() / 1000L;
        long micros = lastMicros.updateAndGet(prev -> Math.max(prev + 1, wallMicros));
        return format(micros);
    }

    private static String format(long micros) {
        long secs = micros / 1_000_000L;
        int us = (int) (micros % 1_000_000L);
        LocalDateTime dt = LocalDateTime.ofInstant(Instant.ofEpochSecond(secs), ZoneOffset.UTC);
        return String.format("%04d-%02d-%02d_%02d-%02d-%02d_%06dZ",
                dt.getYear(), dt.getMonthValue(), dt.getDayOfMonth(),
                dt.getHour(), dt.getMinute(), dt.getSecond(), us);
    }

    static long parseToMicros(String ts) {
        int year  = Integer.parseInt(ts.substring(0, 4));
        int month = Integer.parseInt(ts.substring(5, 7));
        int day   = Integer.parseInt(ts.substring(8, 10));
        int hour  = Integer.parseInt(ts.substring(11, 13));
        int min   = Integer.parseInt(ts.substring(14, 16));
        int sec   = Integer.parseInt(ts.substring(17, 19));
        int us    = Integer.parseInt(ts.substring(20, 26));
        long epochSec = LocalDateTime.of(year, month, day, hour, min, sec)
                .toEpochSecond(ZoneOffset.UTC);
        return epochSec * 1_000_000L + us;
    }

    static String subtractDays(String ts, int days) {
        long micros = parseToMicros(ts) - (long) days * 86400L * 1_000_000L;
        return format(micros);
    }
}
