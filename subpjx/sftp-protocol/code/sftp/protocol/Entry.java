package sftp.protocol;

import java.time.Instant;
import java.util.Objects;

public record Entry(String name, boolean is_dir, Instant mod_time, long byte_size) {
    public Entry {
        Objects.requireNonNull(name, "name");
        Objects.requireNonNull(mod_time, "mod_time");
    }
}
