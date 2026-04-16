use clap::Parser;

use snapbridge::cli::Cli;
use snapbridge::logger;
use snapbridge::run;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    logger::init(cli.log_level.as_level_filter());

    if let Err(error) = run(cli).await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
