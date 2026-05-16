package kitchensync;

import java.time.Instant;

record EntryInfo(String name, boolean directory, Instant modTime, long byteSize) {
}
