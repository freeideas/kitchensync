use std::sync::Arc;
use crate::api::*;

struct CommandLineImpl;

impl CommandLine for CommandLineImpl {
    fn parse(&self, args: Vec<String>) -> CommandLineParseResult {
        unimplemented!()
    }
    fn help_output(&self) -> CommandLineProcessOutput {
        unimplemented!()
    }
    fn validation_error_output( &self, error: &CommandLineValidationError, ) -> CommandLineProcessOutput {
        unimplemented!()
    }
    fn sync_complete_output(&self) -> CommandLineProcessOutput {
        unimplemented!()
    }
    fn should_emit( &self, configured_verbosity: CommandLineVerbosity, message_verbosity: CommandLineVerbosity, ) -> bool {
        unimplemented!()
    }
}

pub fn new() -> std::sync::Arc<dyn CommandLine> {
    Arc::new(CommandLineImpl)
}
