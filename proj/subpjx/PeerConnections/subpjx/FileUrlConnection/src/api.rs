use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileUrlConnectionRequest {
    pub local_peer_root_path: PathBuf,
    pub run_mode: FileUrlConnectionRunMode,
    pub timeout_conn_seconds: u32,
    pub timeout_idle_seconds: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileUrlConnectionRunMode {
    Normal,
    DryRun,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileUrlConnectionHandle {
    pub local_peer_root_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileUrlConnectionFailure {
    pub local_peer_root_path: PathBuf,
    pub reason: FileUrlConnectionFailureReason,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileUrlConnectionFailureReason {
    MissingDirectoryInDryRun,
    PathIsNotDirectory,
    DirectoryStatusUnavailable,
    DirectoryCreationFailed,
}

pub trait FileUrlConnection: Send + Sync {
    /// Establishes one already-parsed `file://` URL attempt for startup.
    ///
    /// One call handles exactly one local peer root path. The caller supplies a
    /// path that has already been parsed, normalized, validated, and selected
    /// for this URL attempt; this operation does not parse peer text, choose a
    /// fallback URL, decide peer identity, emit diagnostics, or decide whether
    /// startup may continue.
    ///
    /// In normal mode, success means the local peer root exists as a directory
    /// before the handle is returned. If the root path is missing, this
    /// operation creates the root directory and all missing parent directories.
    /// If any local filesystem condition prevents the root and required
    /// parents from existing as directories at the end of the attempt, the URL
    /// fails and the returned failure preserves the root path, a structured
    /// reason, and implementation detail for the caller to report.
    ///
    /// In dry-run mode, success requires the local peer root to already exist
    /// as a directory. A missing root directory, a missing required parent, or
    /// an existing non-directory path returns a URL failure. Dry-run mode never
    /// creates the peer root directory or any missing parent directory through
    /// this operation.
    ///
    /// `timeout_conn_seconds` and `timeout_idle_seconds` are accepted only so
    /// callers can pass the same URL-establishment shape used for other peer
    /// URL types. They must not delay, time out, keep alive, retry, or
    /// otherwise change `file://` establishment behavior.
    fn establish_file_url(
        &self,
        request: FileUrlConnectionRequest,
    ) -> Result<FileUrlConnectionHandle, FileUrlConnectionFailure>;
}
