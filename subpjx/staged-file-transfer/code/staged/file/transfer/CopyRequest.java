package staged.file.transfer;

import java.time.Instant;
import java.util.Objects;

public record CopyRequest(
        TransferFilesystem source,
        String source_path,
        TransferFilesystem destination,
        String destination_path,
        Instant winning_mod_time,
        String staging_timestamp,
        String transfer_id,
        int chunk_size,
        int channel_capacity) {
    public CopyRequest {
        Objects.requireNonNull(source, "source");
        Objects.requireNonNull(source_path, "source_path");
        Objects.requireNonNull(destination, "destination");
        Objects.requireNonNull(destination_path, "destination_path");
        Objects.requireNonNull(winning_mod_time, "winning_mod_time");
        Objects.requireNonNull(staging_timestamp, "staging_timestamp");
        Objects.requireNonNull(transfer_id, "transfer_id");
    }
}
