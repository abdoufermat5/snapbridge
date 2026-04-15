use clap::Parser;

use proxsnap::cli::Cli;
use proxsnap::run;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    env_logger::Builder::new()
        .filter_level(cli.log_level.as_level_filter())
        .init();

    if let Err(error) = run(cli).await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
