use crate::api::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

struct ExcludedPathFilterImpl {
    policy: Mutex<RunPolicy>,
}

#[derive(Default)]
struct RunPolicy {
    accepted_paths: HashSet<String>,
    excluded_directory_roots: HashSet<String>,
}

impl ExcludedPathFilter for ExcludedPathFilterImpl {
    fn build_run_policy(
        &self,
        accepted_excludes: Vec<AcceptedExcludePath>,
    ) -> Result<(), ExcludedPathFilterError> {
        let mut accepted_paths = HashSet::new();

        for accepted_exclude in accepted_excludes {
            validate_relative_path(&accepted_exclude.relative_path)?;
            accepted_paths.insert(accepted_exclude.relative_path);
        }

        *self.policy.lock().expect("excluded path policy lock poisoned") = RunPolicy {
            accepted_paths,
            excluded_directory_roots: HashSet::new(),
        };

        Ok(())
    }

    fn decide_path_visibility(
        &self,
        request: PathVisibilityRequest,
    ) -> Result<PathVisibilityDecision, ExcludedPathFilterError> {
        validate_relative_path(&request.relative_path)?;

        if let Some(scope) = built_in_exclusion_scope(&request) {
            return Ok(excluded_decision(request.relative_path, scope));
        }

        let mut policy = self.policy.lock().expect("excluded path policy lock poisoned");

        if is_below_excluded_directory(&request.relative_path, &policy.excluded_directory_roots) {
            return Ok(excluded_decision(
                request.relative_path,
                PathExclusionScope::DirectoryAndDescendants,
            ));
        }

        if policy.accepted_paths.contains(&request.relative_path) {
            let scope = if request.entry_kind == LiveEntryKind::Directory {
                policy
                    .excluded_directory_roots
                    .insert(request.relative_path.clone());
                PathExclusionScope::DirectoryAndDescendants
            } else {
                PathExclusionScope::ExactPath
            };

            return Ok(excluded_decision(request.relative_path, scope));
        }

        Ok(visible_decision(request.relative_path))
    }
}

pub fn new() -> std::sync::Arc<dyn ExcludedPathFilter> {
    Arc::new(ExcludedPathFilterImpl {
        policy: Mutex::new(RunPolicy::default()),
    })
}

fn validate_relative_path(relative_path: &str) -> Result<(), ExcludedPathFilterError> {
    if relative_path.is_empty()
        || relative_path.starts_with('/')
        || relative_path.starts_with('\\')
        || relative_path.contains('\\')
        || relative_path.contains('\0')
        || relative_path.contains(':')
    {
        return Err(ExcludedPathFilterError::InvalidRelativePath(
            relative_path.to_string(),
        ));
    }

    for component in relative_path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(ExcludedPathFilterError::InvalidRelativePath(
                relative_path.to_string(),
            ));
        }
    }

    Ok(())
}

fn built_in_exclusion_scope(request: &PathVisibilityRequest) -> Option<PathExclusionScope> {
    match request.entry_kind {
        LiveEntryKind::SymbolicLinkFile
        | LiveEntryKind::SymbolicLinkDirectory
        | LiveEntryKind::SpecialFile => Some(PathExclusionScope::ExactPath),
        LiveEntryKind::RegularFile | LiveEntryKind::Directory => {
            let components: Vec<&str> = request.relative_path.split('/').collect();

            if components
                .iter()
                .take(components.len().saturating_sub(1))
                .any(|component| is_built_in_directory_name(component))
            {
                return Some(PathExclusionScope::DirectoryAndDescendants);
            }

            if request.entry_kind == LiveEntryKind::Directory
                && components
                    .last()
                    .map_or(false, |component| is_built_in_directory_name(component))
            {
                return Some(PathExclusionScope::DirectoryAndDescendants);
            }

            None
        }
    }
}

fn is_built_in_directory_name(component: &str) -> bool {
    component == ".git" || component == ".kitchensync"
}

fn is_below_excluded_directory(
    relative_path: &str,
    excluded_directory_roots: &HashSet<String>,
) -> bool {
    excluded_directory_roots.iter().any(|root| {
        relative_path.starts_with(root) && relative_path[root.len()..].starts_with('/')
    })
}

fn excluded_decision(
    relative_path: String,
    scope: PathExclusionScope,
) -> PathVisibilityDecision {
    PathVisibilityDecision {
        relative_path,
        exclusion: Some(PathExclusion { scope }),
        eligibility: excluded_eligibility(),
    }
}

fn visible_decision(relative_path: String) -> PathVisibilityDecision {
    PathVisibilityDecision {
        relative_path,
        exclusion: None,
        eligibility: visible_eligibility(),
    }
}

fn excluded_eligibility() -> PathEligibility {
    PathEligibility {
        scan: false,
        recursion: false,
        sync_decision: false,
        copy: false,
        delete: false,
        displace: false,
        snapshot_lookup: false,
        snapshot_update: false,
    }
}

fn visible_eligibility() -> PathEligibility {
    PathEligibility {
        scan: true,
        recursion: true,
        sync_decision: true,
        copy: true,
        delete: true,
        displace: true,
        snapshot_lookup: true,
        snapshot_update: true,
    }
}
