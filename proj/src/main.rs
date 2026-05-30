use std::path::PathBuf;

use kitchensync::cli::CliParseEnv;

fn main() {
    let env = CliParseEnv {
        current_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        current_user: std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "user".to_string()),
    };

    std::process::exit(kitchensync::run_process(std::env::args_os().skip(1), &env));
}
