#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedExcludePath {
    pub relative_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathVisibilityRequest {
    pub relative_path: String,
    pub entry_kind: LiveEntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveEntryKind {
    RegularFile,
    Directory,
    SymbolicLinkFile,
    SymbolicLinkDirectory,
    SpecialFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathVisibilityDecision {
    pub relative_path: String,
    pub exclusion: Option<PathExclusion>,
    pub eligibility: PathEligibility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathExclusion {
    pub scope: PathExclusionScope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathExclusionScope {
    ExactPath,
    DirectoryAndDescendants,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathEligibility {
    pub scan: bool,
    pub recursion: bool,
    pub sync_decision: bool,
    pub copy: bool,
    pub delete: bool,
    pub displace: bool,
    pub snapshot_lookup: bool,
    pub snapshot_update: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExcludedPathFilterError {
    InvalidRelativePath(String),
}

pub trait ExcludedPathFilter: Send + Sync {
    /// Builds the run-local exclusion policy from already accepted command-line
    /// exclude paths.
    ///
    /// The accepted values are treated as relative path matches for this run
    /// only. This method does not parse `-x`, does not accept or reject command
    /// line syntax, and does not read filesystems, transports, snapshots, or
    /// directory listings. The built-in `.kitchensync`, `.git`, symbolic link,
    /// and special-file exclusions remain active regardless of the supplied
    /// command-line excludes.
    ///
    /// A successful call replaces the policy used by later path visibility
    /// checks on this filter handle. If any supplied string is not a valid
    /// relative path for this boundary, the method returns
    /// `ExcludedPathFilterError::InvalidRelativePath` and must not produce
    /// positive eligibility for that invalid path or perform any external
    /// mutation.
    fn build_run_policy(
        &self,
        accepted_excludes: Vec<AcceptedExcludePath>,
    ) -> Result<(), ExcludedPathFilterError>;

    /// Decides whether one live relative path is visible to the rest of the
    /// planning run.
    ///
    /// The caller supplies the live entry classification known from directory
    /// listing facts. A command-line exclude that matches a regular file,
    /// symbolic link file, or special file excludes only that exact path. A
    /// command-line exclude that matches a directory excludes that directory
    /// and every descendant path for the run. After a directory match has been
    /// observed, descendant checks below that directory remain hidden by path
    /// alone and must not require scanning the excluded directory contents.
    ///
    /// Built-in exclusions are always active. Directories named `.kitchensync`
    /// or `.git` are excluded with all descendants. Symbolic link files,
    /// symbolic link directories, and special files are excluded. Built-in
    /// exclusions cannot be overridden by command-line excludes.
    ///
    /// For an excluded path, every eligibility flag in the returned decision
    /// must be false: scan, recursion, sync decision, copy, delete, displace,
    /// snapshot lookup, and snapshot update. For a non-excluded path, true
    /// eligibility flags mean only that the caller may pass the live fact to the
    /// sibling planners or snapshot owner; this child does not make the final
    /// copy, delete, displacement, snapshot, or no-op decision.
    ///
    /// If the relative path is invalid for this boundary, the method returns
    /// `ExcludedPathFilterError::InvalidRelativePath`, must not produce
    /// positive eligibility for the path, and must not perform filesystem
    /// reads, transport listings, snapshot reads, copies, deletes, moves,
    /// snapshot writes, or diagnostics output.
    fn decide_path_visibility(
        &self,
        request: PathVisibilityRequest,
    ) -> Result<PathVisibilityDecision, ExcludedPathFilterError>;
}
