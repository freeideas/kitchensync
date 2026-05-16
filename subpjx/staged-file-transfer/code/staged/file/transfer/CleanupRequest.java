package staged.file.transfer;

import java.util.Objects;

public record CleanupRequest(
        TransferFilesystem filesystem,
        String directory_path,
        String bak_cutoff_exclusive,
        String tmp_cutoff_exclusive) {
    public CleanupRequest {
        Objects.requireNonNull(filesystem, "filesystem");
        Objects.requireNonNull(directory_path, "directory_path");
        Objects.requireNonNull(bak_cutoff_exclusive, "bak_cutoff_exclusive");
        Objects.requireNonNull(tmp_cutoff_exclusive, "tmp_cutoff_exclusive");
    }
}
