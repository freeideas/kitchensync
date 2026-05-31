use crate::{EntryKind, EntryMeta, RelPath, RunConfig};

const BUILT_IN_DIRECTORY_NAMES: &[&str] = &[".kitchensync", ".git"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExcludePredicate {
    command_anchors: Vec<RelPath>,
}

impl ExcludePredicate {
    pub(super) fn from_config(config: &RunConfig) -> Result<Self, ExcludeConfigError> {
        Self::from_anchors(config.excludes.iter().cloned())
    }

    pub(super) fn from_anchors<I>(anchors: I) -> Result<Self, ExcludeConfigError>
    where
        I: IntoIterator<Item = RelPath>,
    {
        let mut command_anchors = Vec::new();
        for anchor in anchors {
            if anchor.as_str().is_empty() {
                return Err(ExcludeConfigError::RootAnchor);
            }
            command_anchors.push(anchor);
        }

        Ok(Self { command_anchors })
    }

    pub(super) fn excludes_candidate(&self, path: &RelPath, metadata: Option<&EntryMeta>) -> bool {
        self.exclusion_reason(path, metadata).is_some()
    }

    pub(super) fn excludes_directory_subtree(&self, path: &RelPath) -> bool {
        path_is_or_is_below_built_in_directory(path)
            || self
                .command_anchors
                .iter()
                .any(|anchor| command_anchor_matches_or_contains(anchor, path))
    }

    fn exclusion_reason(
        &self,
        path: &RelPath,
        metadata: Option<&EntryMeta>,
    ) -> Option<ExclusionReason> {
        if path_has_built_in_directory_ancestor(path) {
            return Some(ExclusionReason::BuiltInDirectory);
        }

        if let Some(metadata) = metadata {
            if !metadata_supported(metadata) {
                return Some(ExclusionReason::UnsupportedMetadata);
            }

            if metadata.kind == EntryKind::Directory && is_built_in_directory_name(&metadata.name) {
                return Some(ExclusionReason::BuiltInDirectory);
            }
        }

        self.command_anchors
            .iter()
            .find_map(|anchor| command_anchor_match(anchor, path))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExcludeConfigError {
    RootAnchor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExclusionReason {
    BuiltInDirectory,
    CommandAnchor,
    CommandDescendant,
    UnsupportedMetadata,
}

fn command_anchor_match(anchor: &RelPath, path: &RelPath) -> Option<ExclusionReason> {
    let anchor = anchor.as_str();
    let path = path.as_str();

    if path == anchor {
        return Some(ExclusionReason::CommandAnchor);
    }

    path.strip_prefix(anchor)
        .filter(|rest| rest.starts_with('/'))
        .map(|_| ExclusionReason::CommandDescendant)
}

fn command_anchor_matches_or_contains(anchor: &RelPath, path: &RelPath) -> bool {
    let anchor = anchor.as_str();
    let path = path.as_str();

    path == anchor
        || path
            .strip_prefix(anchor)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn path_has_built_in_directory_ancestor(path: &RelPath) -> bool {
    let Some((parents, _)) = path.as_str().rsplit_once('/') else {
        return false;
    };

    parents.split('/').any(is_built_in_directory_name)
}

fn path_is_or_is_below_built_in_directory(path: &RelPath) -> bool {
    path.as_str().split('/').any(is_built_in_directory_name)
}

fn is_built_in_directory_name(name: &str) -> bool {
    BUILT_IN_DIRECTORY_NAMES.contains(&name)
}

fn metadata_supported(metadata: &EntryMeta) -> bool {
    match metadata.kind {
        EntryKind::File => metadata.byte_size >= 0,
        EntryKind::Directory => metadata.byte_size == -1,
        EntryKind::SymbolicLink => false,
    }
}
