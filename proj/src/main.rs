fn main() {
    let cli = commandline::new();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output = match cli.parse(args) {
        commandline::CommandLineParseResult::Help => cli.help_output(),
        commandline::CommandLineParseResult::ValidationError(error) => {
            cli.validation_error_output(&error)
        }
        commandline::CommandLineParseResult::Run(_) => cli.sync_complete_output(),
    };
    print!("{}", output.stdout);
    std::process::exit(output.exit_code);
}
