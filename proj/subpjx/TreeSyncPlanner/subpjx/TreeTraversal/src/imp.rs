use std::sync::Arc;
use crate::api::*;

struct TreeTraversalImpl {
    excludedpathfilter: std::sync::Arc<dyn treesyncplanner_treetraversal_excludedpathfilter::ExcludedPathFilter>,
    livedirectorywalk: std::sync::Arc<dyn treesyncplanner_treetraversal_livedirectorywalk::LiveDirectoryWalk>,
}

impl TreeTraversal for TreeTraversalImpl {
    fn traverse_directory(&self, request: TraverseDirectoryRequest) -> TraverseDirectoryResult {
        unimplemented!()
    }
    fn plan_child_recursions( &self, request: ChildRecursionRequest, ) -> Vec<ChildRecursionIntent> {
        unimplemented!()
    }
}

pub fn new(excludedpathfilter: std::sync::Arc<dyn treesyncplanner_treetraversal_excludedpathfilter::ExcludedPathFilter>, livedirectorywalk: std::sync::Arc<dyn treesyncplanner_treetraversal_livedirectorywalk::LiveDirectoryWalk>) -> std::sync::Arc<dyn TreeTraversal> {
    Arc::new(TreeTraversalImpl { excludedpathfilter, livedirectorywalk })
}
