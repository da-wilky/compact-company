use std::{path::PathBuf, process::ExitCode, sync::Arc};

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
    /// Print the API's OpenAPI description
    Openapi,
}

#[tokio::main]
async fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Serve { config } => serve(config).await,
        Command::Openapi => openapi(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(config: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let config = match config {
        Some(path) => Config::load(&path)?,
        None => Config::default(),
    };
    let app = Arc::new(App::open(&config).await?);
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    eprintln!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, compact_company::router(app)).await?;
    Ok(())
}

fn openapi() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", compact_company::openapi().to_pretty_json()?);
    Ok(())
}
