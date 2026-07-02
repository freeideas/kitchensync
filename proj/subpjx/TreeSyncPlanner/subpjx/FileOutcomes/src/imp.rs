use std::sync::Arc;
use crate::api::*;

struct FileOutcomesImpl {
    groupfiledecision: std::sync::Arc<dyn treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecision>,
    peerfileclassification: std::sync::Arc<dyn treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassification>,
}

impl FileOutcomes for FileOutcomesImpl {
    fn classify_peer_file( &self, request: PeerFileClassificationRequest, ) -> Result<PeerFileClassification, FileOutcomesError> {
        unimplemented!()
    }
    fn decide_file_outcome( &self, request: FileOutcomeRequest, ) -> Result<FileOutcomeDecision, FileOutcomesError> {
        unimplemented!()
    }
}

pub fn new(groupfiledecision: std::sync::Arc<dyn treesyncplanner_fileoutcomes_groupfiledecision::GroupFileDecision>, peerfileclassification: std::sync::Arc<dyn treesyncplanner_fileoutcomes_peerfileclassification::PeerFileClassification>) -> std::sync::Arc<dyn FileOutcomes> {
    Arc::new(FileOutcomesImpl { groupfiledecision, peerfileclassification })
}
