package staged.file.transfer;

import java.util.List;
import java.util.Objects;

public record OperationResult(
        OperationStatus status,
        List<String> created_paths,
        List<String> removed_paths,
        String backup_path,
        String temporary_path,
        String final_path,
        TransferError error) {
    public OperationResult {
        Objects.requireNonNull(status, "status");
        created_paths = List.copyOf(created_paths == null ? List.of() : created_paths);
        removed_paths = List.copyOf(removed_paths == null ? List.of() : removed_paths);
    }

    public static OperationResult success(
            List<String> createdPaths,
            List<String> removedPaths,
            String backupPath,
            String temporaryPath,
            String finalPath) {
        return new OperationResult(
                OperationStatus.success,
                createdPaths,
                removedPaths,
                backupPath,
                temporaryPath,
                finalPath,
                null);
    }

    public static OperationResult failed(
            TransferError error,
            List<String> createdPaths,
            List<String> removedPaths,
            String backupPath,
            String temporaryPath,
            String finalPath) {
        return new OperationResult(
                OperationStatus.failed,
                createdPaths,
                removedPaths,
                backupPath,
                temporaryPath,
                finalPath,
                error);
    }

    public static OperationResult partial(
            TransferError error,
            List<String> createdPaths,
            List<String> removedPaths,
            String backupPath,
            String temporaryPath,
            String finalPath) {
        return new OperationResult(
                OperationStatus.partial_success,
                createdPaths,
                removedPaths,
                backupPath,
                temporaryPath,
                finalPath,
                error);
    }
}
