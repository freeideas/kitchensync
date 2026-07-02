use std::sync::Arc;
use crate::api::*;

struct TreeSyncPlannerImpl {
    directoryoutcomes: std::sync::Arc<dyn treesyncplanner_directoryoutcomes::DirectoryOutcomes>,
    fileoutcomes: std::sync::Arc<dyn treesyncplanner_fileoutcomes::FileOutcomes>,
    peerrunroles: std::sync::Arc<dyn treesyncplanner_peerrunroles::PeerRunRoles>,
    treetraversal: std::sync::Arc<dyn treesyncplanner_treetraversal::TreeTraversal>,
    typeconflictoutcomes: std::sync::Arc<dyn treesyncplanner_typeconflictoutcomes::TypeConflictOutcomes>,
}

impl TreeSyncPlanner for TreeSyncPlannerImpl {
    fn decide_startup_roles(&self, request: StartupRoleRequest) -> StartupRoleDecision {
        unimplemented!()
    }
    fn plan_sync_root(&self, request: TreeSyncPlanRequest) -> TreeSyncPlan {
        unimplemented!()
    }
}

pub fn new(directoryoutcomes: std::sync::Arc<dyn treesyncplanner_directoryoutcomes::DirectoryOutcomes>, fileoutcomes: std::sync::Arc<dyn treesyncplanner_fileoutcomes::FileOutcomes>, peerrunroles: std::sync::Arc<dyn treesyncplanner_peerrunroles::PeerRunRoles>, treetraversal: std::sync::Arc<dyn treesyncplanner_treetraversal::TreeTraversal>, typeconflictoutcomes: std::sync::Arc<dyn treesyncplanner_typeconflictoutcomes::TypeConflictOutcomes>) -> std::sync::Arc<dyn TreeSyncPlanner> {
    Arc::new(TreeSyncPlannerImpl { directoryoutcomes, fileoutcomes, peerrunroles, treetraversal, typeconflictoutcomes })
}
