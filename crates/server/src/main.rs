use std::net::SocketAddr;

use clap::{Parser, Subcommand};

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
        #[arg(long, default_value = "127.0.0.1:9440")]
        listen: SocketAddr,
    },
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let Command::Serve { listen } = Cli::parse().command;
    let listener = tokio::net::TcpListener::bind(listen).await?;
    eprintln!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, compact_company::router()).await
}
