use std::sync::Arc;
use treesyncplanner_treetraversal_excludedpathfilter::{
    new, AcceptedExcludePath, ExcludedPathFilter, LiveEntryKind, PathEligibility,
    PathExclusionScope, PathVisibilityDecision, PathVisibilityRequest,
};

fn subject() -> Arc<dyn ExcludedPathFilter> {
    new()
}

fn request(relative_path: &str, entry_kind: LiveEntryKind) -> PathVisibilityRequest {
    PathVisibilityRequest {
        relative_path: relative_path.to_string(),
        entry_kind,
    }
}

fn accepted(relative_path: &str) -> AcceptedExcludePath {
    AcceptedExcludePath {
        relative_path: relative_path.to_string(),
    }
}

fn decide(
    filter: &Arc<dyn ExcludedPathFilter>,
    relative_path: &str,
    entry_kind: LiveEntryKind,
) -> PathVisibilityDecision {
    filter
        .decide_path_visibility(request(relative_path, entry_kind))
        .expect("visibility request should be valid")
}

fn all_excluded_eligibility_is_false(eligibility: PathEligibility) {
    assert!(!eligibility.scan);
    assert!(!eligibility.recursion);
    assert!(!eligibility.sync_decision);
    assert!(!eligibility.copy);
    assert!(!eligibility.delete);
    assert!(!eligibility.displace);
    assert!(!eligibility.snapshot_lookup);
    assert!(!eligibility.snapshot_update);
}

fn assert_excluded(
    decision: PathVisibilityDecision,
    expected_relative_path: &str,
    expected_scope: PathExclusionScope,
) {
    assert_eq!(decision.relative_path, expected_relative_path);
    assert_eq!(
        decision.exclusion.expect("path should be excluded").scope,
        expected_scope
    );
    all_excluded_eligibility_is_false(decision.eligibility);
}

fn assert_visible(decision: PathVisibilityDecision, expected_relative_path: &str) {
    assert_eq!(decision.relative_path, expected_relative_path);
    assert_eq!(decision.exclusion, None);
    assert!(decision.eligibility.scan);
    assert!(decision.eligibility.recursion);
    assert!(decision.eligibility.sync_decision);
    assert!(decision.eligibility.copy);
    assert!(decision.eligibility.delete);
    assert!(decision.eligibility.displace);
    assert!(decision.eligibility.snapshot_lookup);
    assert!(decision.eligibility.snapshot_update);
}

#[test]
fn accepted_exclude_matching_file_excludes_only_that_file_path() {
    let filter = subject();
    filter
        .build_run_policy(vec![accepted("notes.txt")])
        .expect("accepted exclude should build policy");

    assert_excluded(
        decide(&filter, "notes.txt", LiveEntryKind::RegularFile),
        "notes.txt",
        PathExclusionScope::ExactPath,
    );
    assert_visible(
        decide(&filter, "notes.txt/child.txt", LiveEntryKind::RegularFile),
        "notes.txt/child.txt",
    );
}

#[test]
fn accepted_exclude_matching_directory_excludes_directory_and_later_descendants() {
    let filter = subject();
    filter
        .build_run_policy(vec![accepted("cache")])
        .expect("accepted exclude should build policy");

    assert_excluded(
        decide(&filter, "cache", LiveEntryKind::Directory),
        "cache",
        PathExclusionScope::DirectoryAndDescendants,
    );
    assert_excluded(
        decide(&filter, "cache/nested/file.txt", LiveEntryKind::RegularFile),
        "cache/nested/file.txt",
        PathExclusionScope::DirectoryAndDescendants,
    );
}

#[test]
fn kitchensync_and_git_directories_are_always_excluded_with_descendants() {
    let filter = subject();
    filter
        .build_run_policy(vec![accepted("ordinary.txt")])
        .expect("accepted exclude should build policy");

    assert_excluded(
        decide(&filter, ".kitchensync", LiveEntryKind::Directory),
        ".kitchensync",
        PathExclusionScope::DirectoryAndDescendants,
    );
    assert_excluded(
        decide(&filter, "project/.git/config", LiveEntryKind::RegularFile),
        "project/.git/config",
        PathExclusionScope::DirectoryAndDescendants,
    );
}

#[test]
fn symbolic_links_and_special_files_are_always_excluded() {
    let filter = subject();
    filter
        .build_run_policy(Vec::new())
        .expect("empty policy should build");

    assert_excluded(
        decide(&filter, "link-file", LiveEntryKind::SymbolicLinkFile),
        "link-file",
        PathExclusionScope::ExactPath,
    );
    assert_excluded(
        decide(&filter, "link-dir", LiveEntryKind::SymbolicLinkDirectory),
        "link-dir",
        PathExclusionScope::ExactPath,
    );
    assert_excluded(
        decide(&filter, "device", LiveEntryKind::SpecialFile),
        "device",
        PathExclusionScope::ExactPath,
    );
}

#[test]
fn built_in_exclusions_remain_active_when_command_line_excludes_are_supplied() {
    let filter = subject();
    filter
        .build_run_policy(vec![accepted("some-file.txt"), accepted("some-dir")])
        .expect("accepted excludes should build policy");

    assert_excluded(
        decide(&filter, ".git", LiveEntryKind::Directory),
        ".git",
        PathExclusionScope::DirectoryAndDescendants,
    );
    assert_excluded(
        decide(&filter, "symlink", LiveEntryKind::SymbolicLinkFile),
        "symlink",
        PathExclusionScope::ExactPath,
    );
}
