use std::sync::Arc;
use crate::api::*;

struct FormatRulesImpl;

impl FormatRules for FormatRulesImpl {
    fn normalize_peer_identity( &self, request: FormatRulesPeerIdentityRequest, ) -> Result<String, FormatRulesValidationError> {
        unimplemented!()
    }
    fn validate_relative_path( &self, path: &str, ) -> Result<String, FormatRulesValidationError> {
        unimplemented!()
    }
    fn snapshot_path_ids( &self, relative_path: &str, ) -> Result<FormatRulesSnapshotPathIds, FormatRulesValidationError> {
        unimplemented!()
    }
    fn parse_timestamp( &self, timestamp: &str, ) -> Result<FormatRulesTimestamp, FormatRulesValidationError> {
        unimplemented!()
    }
    fn format_timestamp(&self, timestamp: SystemTime) -> FormatRulesTimestamp {
        unimplemented!()
    }
    fn current_timestamp(&self) -> FormatRulesTimestamp {
        unimplemented!()
    }
    fn timestamp_text(&self, timestamp: &FormatRulesTimestamp) -> String {
        unimplemented!()
    }
    fn timestamp_system_time(&self, timestamp: &FormatRulesTimestamp) -> SystemTime {
        unimplemented!()
    }
    fn confirmed_absence_deleted_time( &self, existing_last_seen: &FormatRulesTimestamp, existing_deleted_time: Option<&FormatRulesTimestamp>, ) -> FormatRulesDeletionEstimateUpdate {
        unimplemented!()
    }
    fn displacement_deleted_time( &self, existing_last_seen: &FormatRulesTimestamp, ) -> FormatRulesTimestamp {
        unimplemented!()
    }
    fn displacement_cascade_deleted_time( &self, displaced_deleted_time: &FormatRulesTimestamp, ) -> FormatRulesTimestamp {
        unimplemented!()
    }
    fn bak_directory_path( &self, parent_relative_path: Option<&str>, timestamp: &FormatRulesTimestamp, ) -> Result<String, FormatRulesValidationError> {
        unimplemented!()
    }
    fn tmp_directory_path(&self, timestamp: &FormatRulesTimestamp) -> String {
        unimplemented!()
    }
    fn user_swap_paths( &self, parent_relative_path: Option<&str>, target_basename: &str, ) -> Result<FormatRulesUserSwapPaths, FormatRulesValidationError> {
        unimplemented!()
    }
    fn snapshot_swap_paths(&self) -> FormatRulesSnapshotSwapPaths {
        unimplemented!()
    }
    fn file_mod_times_same( &self, current_mod_time: &FormatRulesTimestamp, snapshot_mod_time: &FormatRulesTimestamp, ) -> bool {
        unimplemented!()
    }
    fn peer_mod_time_tied_with_max( &self, candidate_mod_time: &FormatRulesTimestamp, max_mod_time: &FormatRulesTimestamp, ) -> bool {
        unimplemented!()
    }
    fn peer_mod_time_older_than_max( &self, candidate_mod_time: &FormatRulesTimestamp, max_mod_time: &FormatRulesTimestamp, ) -> bool {
        unimplemented!()
    }
    fn deletion_estimate_wins_over_file_mod_time( &self, deletion_estimate: &FormatRulesTimestamp, file_mod_time: &FormatRulesTimestamp, ) -> bool {
        unimplemented!()
    }
    fn absent_unconfirmed_file_counts_as_deletion( &self, last_seen: &FormatRulesTimestamp, max_live_file_mod_time: &FormatRulesTimestamp, ) -> bool {
        unimplemented!()
    }
    fn directory_live_file_timestamp_evidence( &self, live_file_mod_times: &[FormatRulesTimestamp], ) -> Option<FormatRulesTimestamp> {
        unimplemented!()
    }
    fn directory_deletion_estimate_newer_than_live_file_evidence( &self, deletion_estimate: &FormatRulesTimestamp, newest_live_file_mod_time: &FormatRulesTimestamp, ) -> bool {
        unimplemented!()
    }
}

pub fn new() -> std::sync::Arc<dyn FormatRules> {
    Arc::new(FormatRulesImpl)
}
