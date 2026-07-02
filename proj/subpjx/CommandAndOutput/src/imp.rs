use std::sync::Arc;
use crate::api::*;

struct CommandAndOutputImpl {
    globalargumentparser: std::sync::Arc<dyn commandandoutput_globalargumentparser::GlobalArgumentParser>,
    peerargumentparser: std::sync::Arc<dyn commandandoutput_peerargumentparser::PeerArgumentParser>,
    peeridentitynormalizer: std::sync::Arc<dyn commandandoutput_peeridentitynormalizer::PeerIdentityNormalizer>,
    stdoutreporter: std::sync::Arc<dyn commandandoutput_stdoutreporter::StdoutReporter>,
}

impl CommandAndOutput for CommandAndOutputImpl {
    fn parse_command( &self, args: Vec<String>, current_working_directory: PathBuf, current_os_username: String, ) -> CommandParseResult {
        unimplemented!()
    }
    fn normalize_peer_identity( &self, target: PeerLocation, current_working_directory: PathBuf, current_os_username: String, ) -> Result<String, PeerIdentityError> {
        unimplemented!()
    }
    fn write_output(&self, verbosity: Verbosity, event: OutputEvent) {
        unimplemented!()
    }
}

pub fn new(globalargumentparser: std::sync::Arc<dyn commandandoutput_globalargumentparser::GlobalArgumentParser>, peerargumentparser: std::sync::Arc<dyn commandandoutput_peerargumentparser::PeerArgumentParser>, peeridentitynormalizer: std::sync::Arc<dyn commandandoutput_peeridentitynormalizer::PeerIdentityNormalizer>, stdoutreporter: std::sync::Arc<dyn commandandoutput_stdoutreporter::StdoutReporter>) -> std::sync::Arc<dyn CommandAndOutput> {
    Arc::new(CommandAndOutputImpl { globalargumentparser, peerargumentparser, peeridentitynormalizer, stdoutreporter })
}
