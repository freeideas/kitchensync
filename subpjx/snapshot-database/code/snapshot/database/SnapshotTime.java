package snapshot.database;

import java.time.DateTimeException;
import java.time.LocalDateTime;
import java.time.ZoneOffset;
import java.time.format.DateTimeFormatter;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public record SnapshotTime(String value) implements Comparable<SnapshotTime> {
    private static final Pattern PATTERN = Pattern.compile(
            "^(\\d{4})-(\\d{2})-(\\d{2})_(\\d{2})-(\\d{2})-(\\d{2})_(\\d{6})Z$");

    public SnapshotTime {
        validate(value);
    }

    public static SnapshotTime of(String value) {
        return new SnapshotTime(value);
    }

    static SnapshotTime fromEpochMicros(long epochMicros) {
        long seconds = Math.floorDiv(epochMicros, 1_000_000L);
        int micros = (int) Math.floorMod(epochMicros, 1_000_000L);
        LocalDateTime dateTime = LocalDateTime.ofEpochSecond(seconds, micros * 1_000, ZoneOffset.UTC);
        return new SnapshotTime(dateTime.format(DateTimeFormatter.ofPattern("yyyy-MM-dd_HH-mm-ss_"))
                + "%06dZ".formatted(micros));
    }

    static long toEpochMicros(SnapshotTime time) {
        Matcher matcher = PATTERN.matcher(time.value);
        if (!matcher.matches()) {
            throw invalid();
        }
        LocalDateTime dateTime = LocalDateTime.of(
                Integer.parseInt(matcher.group(1)),
                Integer.parseInt(matcher.group(2)),
                Integer.parseInt(matcher.group(3)),
                Integer.parseInt(matcher.group(4)),
                Integer.parseInt(matcher.group(5)),
                Integer.parseInt(matcher.group(6)),
                Integer.parseInt(matcher.group(7)) * 1_000);
        return Math.addExact(Math.multiplyExact(dateTime.toEpochSecond(ZoneOffset.UTC), 1_000_000L),
                Integer.parseInt(matcher.group(7)));
    }

    private static void validate(String value) {
        if (value == null) {
            throw invalid();
        }
        Matcher matcher = PATTERN.matcher(value);
        if (!matcher.matches()) {
            throw invalid();
        }
        try {
            LocalDateTime.of(
                    Integer.parseInt(matcher.group(1)),
                    Integer.parseInt(matcher.group(2)),
                    Integer.parseInt(matcher.group(3)),
                    Integer.parseInt(matcher.group(4)),
                    Integer.parseInt(matcher.group(5)),
                    Integer.parseInt(matcher.group(6)),
                    Integer.parseInt(matcher.group(7)) * 1_000);
        } catch (DateTimeException | ArithmeticException e) {
            throw invalid();
        }
    }

    private static SnapshotDatabaseException invalid() {
        return new SnapshotDatabaseException("invalid_timestamp", "invalid timestamp");
    }

    @Override
    public int compareTo(SnapshotTime other) {
        return value.compareTo(other.value);
    }

    @Override
    public String toString() {
        return value;
    }
}
