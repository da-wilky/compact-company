use std::{io::Write, path::PathBuf, process::ExitCode, sync::Arc};

use clap::{Parser, Subcommand};
use compact_company::{
    App,
    config::Config,
    documents,
    render::{self, Layout},
};

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
    /// Render sample data with a layout and write the PDF to stdout, for writing layouts
    Sample {
        /// Directory with `layout.typ` and optional `assets/` and `fonts/`; the built-in layout
        /// without it
        #[arg(long)]
        layout: Option<PathBuf>,
        #[arg(long, default_value = "invoice")]
        kind: String,
        #[arg(long, default_value = "de")]
        language: String,
        #[arg(long)]
        small_business: bool,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Serve { config } => serve(config).await,
        Command::Openapi => openapi(),
        Command::Sample {
            layout,
            kind,
            language,
            small_business,
        } => sample(layout, &kind, &language, small_business),
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
    if app.setup_required().await? {
        eprintln!(
            "no users yet; set up this instance at {}setup#{}",
            app.public_url, app.setup_token
        );
    }
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    eprintln!("listening on http://{}", listener.local_addr()?);
    axum::serve(
        listener,
        compact_company::router(app).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn openapi() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", compact_company::openapi().to_pretty_json()?);
    Ok(())
}

fn sample(
    dir: Option<PathBuf>,
    kind: &str,
    language: &str,
    small_business: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let kind = serde_json::from_value(serde_json::Value::from(kind))?;
    let language = serde_json::from_value(serde_json::Value::from(language))?;
    let layout = match dir {
        Some(dir) => Layout::from_dir(&dir)?,
        None => Layout::built_in(),
    };
    let data = documents::sample(kind, language, small_business);
    let rendered = render::render(&layout, &data, &[])?;
    for key in render::missing(&data, &rendered) {
        eprintln!("not shown: {key}");
    }
    std::io::stdout().write_all(&rendered.pdf()?)?;
    Ok(())
}
