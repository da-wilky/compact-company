use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Attachment, Mailbox, MultiPart, SinglePart, header::ContentType},
    transport::smtp::authentication::Credentials,
};

use crate::{
    api::customers::Language,
    config::{Smtp, SmtpSecurity},
    documents::Kind,
};

pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    bcc: Option<Mailbox>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid smtp settings: {0}")]
    Config(String),
}

/// A file to send along.
pub struct File {
    pub name: String,
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

impl Mailer {
    pub fn new(config: &Smtp) -> Result<Self, Error> {
        let invalid = |e: &dyn std::fmt::Display| Error::Config(e.to_string());
        let builder = match config.security {
            SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
                .map_err(|e| invalid(&e))?,
            SmtpSecurity::Starttls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
                    .map_err(|e| invalid(&e))?
            }
            SmtpSecurity::None => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
            }
        };
        let builder = builder.port(config.port);
        let builder = match (&config.username, &config.password) {
            (Some(user), Some(password)) => {
                builder.credentials(Credentials::new(user.clone(), password.clone()))
            }
            _ => builder,
        };
        Ok(Self {
            transport: builder.build(),
            from: config.from.parse().map_err(|e| invalid(&e))?,
            bcc: config
                .bcc
                .as_deref()
                .map(str::parse)
                .transpose()
                .map_err(|e| invalid(&e))?,
        })
    }

    /// Sends a plain-text mail with files; the error is what the server answered.
    pub async fn send(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        files: Vec<File>,
    ) -> Result<(), String> {
        let to: Mailbox = to
            .parse()
            .map_err(|e: lettre::address::AddressError| e.to_string())?;
        let mut builder = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(subject);
        if let Some(bcc) = &self.bcc {
            builder = builder.bcc(bcc.clone());
        }
        let mut parts = MultiPart::mixed().singlepart(SinglePart::plain(body.to_owned()));
        for file in files {
            let content_type =
                ContentType::parse(file.content_type).expect("the app's content types parse");
            parts = parts.singlepart(Attachment::new(file.name).body(file.bytes, content_type));
        }
        let message = builder.multipart(parts).map_err(|e| e.to_string())?;
        self.transport
            .send(message)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// The default subject and body for sending a document.
pub fn default_text(
    kind: Kind,
    number: &str,
    language: Language,
    business: &str,
) -> (String, String) {
    let de = language == Language::De;
    let name = match (kind, de) {
        (Kind::Quote, true) => "Angebot",
        (Kind::Quote, false) => "Quote",
        (Kind::OrderConfirmation, true) => "Auftragsbestätigung",
        (Kind::OrderConfirmation, false) => "Order confirmation",
        (Kind::Invoice, true) => "Rechnung",
        (Kind::Invoice, false) => "Invoice",
        (Kind::Cancellation, true) => "Stornorechnung",
        (Kind::Cancellation, false) => "Cancellation invoice",
        (Kind::DeliveryNote, true) => "Lieferschein",
        (Kind::DeliveryNote, false) => "Delivery note",
        (Kind::AcceptanceReport, true) => "Abnahmeprotokoll",
        (Kind::AcceptanceReport, false) => "Acceptance report",
        (Kind::Dunning, true) => "Mahnung",
        (Kind::Dunning, false) => "Payment reminder",
    };
    let subject = format!("{name} {number}");
    let body = if de {
        format!(
            "Guten Tag,\n\nim Anhang finden Sie {subject}.\n\nMit freundlichen Grüßen\n{business}\n"
        )
    } else {
        format!("Hello,\n\nplease find attached {subject}.\n\nKind regards\n{business}\n")
    };
    (subject, body)
}
