use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};
use compact_company::{App, config::Config};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the app over HTTP
    Serve {
        /// TOML config; without it, defaults apply
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    match serve().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let Command::Serve { config } = Cli::parse().command;
    let config = match config {
        Some(path) => Config::load(&path)?,
        None => Config::default(),
    };
    let _app = App::open(&config).await?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    eprintln!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, compact_company::router()).await?;
    Ok(())
}
