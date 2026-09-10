mod cli;
mod config;
mod engine;
mod ignore;
mod manifest;
mod output;
mod transport;
mod util;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match cli::parse(&args) {
        config::Parsed::Help => {
            output::raw(cli::HELP_TEXT);
            0
        }
        config::Parsed::Error(msg) => {
            output::line(&msg);
            output::raw(cli::HELP_TEXT);
            1
        }
        config::Parsed::Run(cfg) => {
            output::set_level(cfg.verbosity);
            engine::run(cfg)
        }
    };
    std::process::exit(code);
}
