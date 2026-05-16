package staged.file.transfer;

import java.util.Objects;

public record DisplaceRequest(
        TransferFilesystem filesystem,
        String path,
        String staging_timestamp) {
    public DisplaceRequest {
        Objects.requireNonNull(filesystem, "filesystem");
        Objects.requireNonNull(path, "path");
        Objects.requireNonNull(staging_timestamp, "staging_timestamp");
    }
}
