use clap::{CommandFactory, FromArgMatches, Parser};

/// postgame: bot-free, local-first conversation intelligence.
#[derive(Parser)]
#[command(name = "pg")]
struct Cli {}

fn version() -> String {
    format!(
        "{} (PGAudio {})",
        env!("CARGO_PKG_VERSION"),
        pg_capture::shim_version()
    )
}

fn main() {
    let matches = Cli::command().version(version()).get_matches();
    Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
}
