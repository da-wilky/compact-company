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
    reply_to: Option<Mailbox>,
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
            reply_to: config
                .reply_to
                .as_deref()
                .map(str::parse)
                .transpose()
                .map_err(|e| invalid(&e))?,
        })
    }

    /// Sends a plain-text mail with files, and a copy to the configured BCC; the error is what
    /// the server answered.
    pub async fn send(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        files: Vec<File>,
    ) -> Result<(), String> {
        self.deliver(to, subject, body, files, true).await
    }

    /// Sends a plain-text note to the business itself, without the BCC copy.
    pub async fn notify(&self, to: &str, subject: &str, body: &str) -> Result<(), String> {
        self.deliver(to, subject, body, Vec::new(), false).await
    }

    async fn deliver(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        files: Vec<File>,
        copy: bool,
    ) -> Result<(), String> {
        let message = self.message(to, subject, body, files, copy)?;
        self.transport
            .send(message)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn message(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        files: Vec<File>,
        copy: bool,
    ) -> Result<Message, String> {
        let to: Mailbox = to
            .parse()
            .map_err(|e: lettre::address::AddressError| e.to_string())?;
        let mut builder = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(subject);
        if let Some(bcc) = self.bcc.as_ref().filter(|_| copy) {
            builder = builder.bcc(bcc.clone());
        }
        if let Some(reply_to) = &self.reply_to {
            builder = builder.reply_to(reply_to.clone());
        }
        let mut parts = MultiPart::mixed().singlepart(SinglePart::plain(body.to_owned()));
        for file in files {
            let content_type =
                ContentType::parse(file.content_type).expect("the app's content types parse");
            parts = parts.singlepart(Attachment::new(file.name).body(file.bytes, content_type));
        }
        builder.multipart(parts).map_err(|e| e.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_go_to_reply_to_and_notes_skip_the_copy() {
        let mailer = Mailer {
            transport: AsyncSmtpTransport::<Tokio1Executor>::unencrypted_localhost(),
            from: "Example GmbH <rechnung@example.com>".parse().unwrap(),
            bcc: Some("archiv@example.com".parse().unwrap()),
            reply_to: Some("Erika Muster <kontakt@example.com>".parse().unwrap()),
        };
        let message = |copy| {
            mailer
                .message("kunde@example.com", "Rechnung", "Text", Vec::new(), copy)
                .unwrap()
        };
        let headers = message(true).headers().to_string();
        assert!(
            headers.contains(r#"Reply-To: "Erika Muster" <kontakt@example.com>"#),
            "{headers}"
        );
        assert_eq!(message(true).envelope().to().len(), 2);
        assert_eq!(message(false).envelope().to().len(), 1);
    }
}
