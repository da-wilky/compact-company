use std::{io::Write, path::PathBuf, process::ExitCode, sync::Arc};

use clap::{Parser, Subcommand};
use compact_company::{
    App, api,
    api::customers::Language,
    config::Config,
    deadlines,
    documents::{self, InvoiceType},
    einvoice,
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
        /// A document kind, or `partial` or `final` for those invoices
        #[arg(long, default_value = "invoice")]
        kind: String,
        #[arg(long, default_value = "de")]
        language: String,
        #[arg(long)]
        small_business: bool,
    },
    /// Write the e-invoice XML of sample data to stdout, for checking it with a validator
    Einvoice {
        /// A document kind, or `partial` or `final` for those invoices
        #[arg(long, default_value = "invoice")]
        kind: String,
        #[arg(long)]
        small_business: bool,
        /// XRechnung instead of the XML embedded as ZUGFeRD
        #[arg(long)]
        xrechnung: bool,
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
        Command::Einvoice {
            kind,
            small_business,
            xrechnung,
        } => einvoice(&kind, small_business, xrechnung),
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
    tokio::spawn(remind(app.clone()));
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    eprintln!("listening on http://{}", listener.local_addr()?);
    axum::serve(
        listener,
        compact_company::router(app).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Mails deadline reminders and drafts recurring invoices, checking every hour.
async fn remind(app: Arc<App>) {
    loop {
        let today = app
            .settings
            .business()
            .await
            .map_err(|e| e.to_string())
            .and_then(|b| api::documents::today(&b).map_err(|e| e.message));
        match today {
            Ok(today) => {
                if let Err(e) = deadlines::remind(&app, today).await {
                    eprintln!("deadline reminders failed: {e}");
                }
                if let Err(e) = api::recurring::draft_due(&app, today).await {
                    eprintln!("drafting recurring invoices failed: {}", e.message);
                }
            }
            Err(e) => eprintln!("deadline reminders failed: {e}"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
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
    let language = serde_json::from_value(serde_json::Value::from(language))?;
    let layout = match dir {
        Some(dir) => Layout::from_dir(&dir)?,
        None => Layout::built_in(),
    };
    let data = sample_data(kind, language, small_business)?;
    let rendered = render::render(&layout, &data, &[])?;
    for key in render::missing(&data, &rendered) {
        eprintln!("not shown: {key}");
    }
    std::io::stdout().write_all(&rendered.pdf()?)?;
    Ok(())
}

fn einvoice(
    kind: &str,
    small_business: bool,
    xrechnung: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = sample_data(kind, Language::De, small_business)?;
    let flavour = if xrechnung {
        einvoice::Flavour::Xrechnung
    } else {
        einvoice::Flavour::Zugferd
    };
    std::io::stdout().write_all(&einvoice::cii(&data, flavour))?;
    Ok(())
}

/// Sample data of a document kind, or of a `partial` or `final` invoice.
fn sample_data(
    kind: &str,
    language: Language,
    small_business: bool,
) -> Result<documents::Data, serde_json::Error> {
    let invoice_type: Option<InvoiceType> = serde_json::from_value(kind.into()).ok();
    Ok(match invoice_type {
        Some(invoice_type) => documents::sample_invoice(invoice_type, language, small_business),
        None => documents::sample(
            serde_json::from_value(kind.into())?,
            language,
            small_business,
        ),
    })
}
