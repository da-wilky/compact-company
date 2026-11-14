//! The process documentation (GoBD Rz. 150–155): what the instance knows is generated, the rest
//! comes from the owner's answers; every change makes a new version (ADR 0027).

use std::collections::{BTreeMap, BTreeSet};

use axum::body::Bytes;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use crate::{
    App,
    api::customers::Language,
    auth::now,
    config::Config,
    files::{self, ContentHash},
    render,
    settings::{Business, Taxation},
};

const LAYOUT: &str = include_str!("../typst/process.typ");

/// The four parts of GoBD Rz. 151.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = ProcessPart)]
pub enum Part {
    General,
    User,
    Technical,
    Operations,
}

const PARTS: [Part; 4] = [Part::General, Part::User, Part::Technical, Part::Operations];

/// A text in German and English.
#[derive(Debug, Clone, Copy)]
pub struct Text {
    de: &'static str,
    en: &'static str,
}

impl Text {
    pub fn get(self, language: Language) -> &'static str {
        match language {
            Language::De => self.de,
            Language::En => self.en,
        }
    }
}

const fn t(de: &'static str, en: &'static str) -> Text {
    Text { de, en }
}

pub struct Question {
    pub key: &'static str,
    pub part: Part,
    pub question: Text,
    /// Why it is asked and what a good answer covers.
    pub help: Text,
    /// An answer that fits a business of one person.
    pub suggestion: Text,
}

pub const QUESTIONS: &[Question] = &[
    Question {
        key: "activity",
        part: Part::General,
        question: t("Was tut das Unternehmen?", "What does the business do?"),
        help: t(
            "Tätigkeit, Kunden und Umfang in wenigen Sätzen. Daraus sieht eine Prüferin, welche Geschäftsvorfälle zu erwarten sind.",
            "Its work, customers and size in a few sentences. An auditor reads from it which transactions to expect.",
        ),
        suggestion: t(
            "Softwareentwicklung und Beratung für Geschäftskunden, ausgeübt vom Inhaber allein. Abgerechnet wird nach Aufwand oder zum Festpreis; Rechnungen gehen per E-Mail.",
            "Software development and consulting for business customers, done by the owner alone. Work is billed by effort or at a fixed price; invoices go out by email.",
        ),
    },
    Question {
        key: "responsibilities",
        part: Part::General,
        question: t("Wer bucht, und wer prüft?", "Who books, and who checks?"),
        help: t(
            "Wer Belege erfasst und bucht, wer Rechnungen ausstellt, und wie die Buchungen kontrolliert werden (GoBD Rz. 102).",
            "Who enters and books receipts, who issues invoices, and how the bookings are checked (GoBD Rz. 102).",
        ),
        suggestion: t(
            "Der Inhaber stellt alle Rechnungen aus, erfasst und bucht alle Belege selbst und gleicht monatlich die Bankumsätze und offenen Posten ab. Weitere Personen buchen nicht.",
            "The owner issues every invoice, enters and books every receipt and reconciles the bank transactions and open items each month. Nobody else books.",
        ),
    },
    Question {
        key: "accountant",
        part: Part::General,
        question: t(
            "Was übernimmt eine Steuerberatung?",
            "What does a tax adviser do?",
        ),
        help: t(
            "Ob eine Steuerberatung mitarbeitet, welche Daten sie bekommt (etwa den DATEV-Export) und was sie erstellt.",
            "Whether a tax adviser takes part, which data they get (such as the DATEV export) and what they prepare.",
        ),
        suggestion: t(
            "Keine Steuerberatung. Umsatzsteuer-Voranmeldungen und EÜR erstellt der Inhaber aus den Zahlen der App und gibt sie in Mein ELSTER ein.",
            "No tax adviser. The owner prepares the VAT returns and the EÜR from the app's figures and enters them in Mein ELSTER.",
        ),
    },
    Question {
        key: "receipt_arrival",
        part: Part::User,
        question: t(
            "Wie kommen Belege ins Unternehmen?",
            "How do receipts reach the business?",
        ),
        help: t(
            "Wege der Eingangsrechnungen und Quittungen (E-Mail, Portal, Papier) und wann sie in die App kommen.",
            "How incoming invoices and receipts arrive (email, portal, paper) and when they are put into the app.",
        ),
        suggestion: t(
            "Eingangsrechnungen kommen meist per E-Mail als PDF oder E-Rechnung und werden innerhalb einer Woche hochgeladen. Quittungen auf Papier werden mit dem Smartphone fotografiert und hochgeladen.",
            "Incoming invoices mostly arrive by email as PDF or e-invoice and are uploaded within a week. Paper receipts are photographed with a phone and uploaded.",
        ),
    },
    Question {
        key: "capture",
        part: Part::User,
        question: t(
            "Wer erfasst Papierbelege, wann und welche?",
            "Who captures paper receipts, when, and which?",
        ),
        help: t(
            "Die Organisationsanweisung zum Scannen nach GoBD Rz. 136: wer erfasst, wann, und welche Belege (alle oder eine bestimmte Auswahl).",
            "The scanning instruction of GoBD Rz. 136: who captures, when, and which documents (all of them or a defined selection).",
        ),
        suggestion: t(
            "Der Inhaber erfasst alle Papierbelege selbst, vollständig und ohne Auswahl, spätestens zum Ende des Monats, in dem sie eingegangen sind.",
            "The owner captures every paper receipt personally, all of them without selection, by the end of the month they arrived in.",
        ),
    },
    Question {
        key: "capture_check",
        part: Part::User,
        question: t(
            "Wie wird geprüft, dass das Bild dem Papier entspricht?",
            "How is the image checked against the paper?",
        ),
        help: t(
            "Bildliche und inhaltliche Übereinstimmung, Qualitätskontrolle auf Lesbarkeit und Vollständigkeit, und wie Fehler festgehalten werden (GoBD Rz. 136).",
            "Whether image and content match, the quality check for legibility and completeness, and how errors are recorded (GoBD Rz. 136).",
        ),
        suggestion: t(
            "Nach dem Hochladen vergleicht der Inhaber das Bild mit dem Papier: alle Seiten, Beträge, Datum und Aussteller lesbar. Eine unlesbare Aufnahme wird wiederholt; ein Fehler wird in der Notiz des Belegs vermerkt.",
            "After uploading, the owner compares the image with the paper: every page, amount, date and issuer legible. An illegible image is taken again; an error is noted on the receipt.",
        ),
    },
    Question {
        key: "paper",
        part: Part::User,
        question: t(
            "Was geschieht mit dem Papier nach dem Erfassen?",
            "What happens to the paper after capture?",
        ),
        help: t(
            "Ob Papierbelege nach dem Erfassen vernichtet werden, und welche im Original bleiben, weil ihr Papier Beweiskraft hat (GoBD Rz. 139–140).",
            "Whether paper is destroyed after capture, and which originals are kept because the paper itself is evidence (GoBD Rz. 139–140).",
        ),
        suggestion: t(
            "Papierbelege werden nach der Prüfung bis zum Ende des Folgemonats aufbewahrt und dann vernichtet. Im Original bleiben Belege, deren Papier Beweiskraft hat, etwa Urkunden oder Zollbelege.",
            "Paper receipts are kept until the end of the following month after the check and then destroyed. Originals whose paper is evidence, such as deeds or customs documents, are kept.",
        ),
    },
    Question {
        key: "bank",
        part: Part::User,
        question: t(
            "Welche Bankkonten gibt es, und wie kommen die Umsätze in die App?",
            "Which bank accounts are used, and how do their transactions reach the app?",
        ),
        help: t(
            "Konten mit Geschäftsvorfällen, auch ein privat geführtes, und wie oft die Kontoauszüge importiert werden.",
            "Accounts with business transactions, a private one included, and how often their statements are imported.",
        ),
        suggestion: t(
            "Ein Geschäftskonto. Die Umsätze werden monatlich als CAMT.053 oder CSV heruntergeladen, importiert und den Rechnungen und Belegen zugeordnet.",
            "One business account. Its transactions are downloaded monthly as CAMT.053 or CSV, imported and matched to invoices and receipts.",
        ),
    },
    Question {
        key: "checks",
        part: Part::Operations,
        question: t(
            "Wie oft werden Sicherung und Spiegelung geprüft?",
            "How often are the backup and the mirror checked?",
        ),
        help: t(
            "Wer nachsieht, dass Sicherungen gelingen und sich wiederherstellen lassen, und wie oft.",
            "Who checks that backups succeed and can be restored, and how often.",
        ),
        suggestion: t(
            "Monatlich sieht der Inhaber die letzte Sicherung in der App nach und öffnet Stichproben in der Spiegelung. Einmal im Jahr wird eine Sicherung auf einer Testinstanz wiederhergestellt.",
            "Each month the owner looks at the last backup in the app and opens samples in the mirror. Once a year a backup is restored onto a test instance.",
        ),
    },
    Question {
        key: "access",
        part: Part::Operations,
        question: t(
            "Wer hat Zugang zum Server und zur Konfiguration?",
            "Who has access to the server and its configuration?",
        ),
        help: t(
            "Wer den Server verwaltet, wo er steht, und wie der Zugang geschützt ist.",
            "Who administers the server, where it is, and how access is protected.",
        ),
        suggestion: t(
            "Nur der Inhaber. Der Server steht in den eigenen Geschäftsräumen; angemeldet wird per SSH-Schlüssel, die Konfiguration liegt versioniert in einem eigenen Repository.",
            "Only the owner. The server is on the business's own premises; access is by SSH key, and the configuration is versioned in the owner's repository.",
        ),
    },
];

/// What the instance knows, by key, with its label.
const FACTS: &[(&str, Part, Text)] = &[
    ("business", Part::General, t("Unternehmen", "Business")),
    ("taxation", Part::General, t("Besteuerung", "Taxation")),
    (
        "chart",
        Part::General,
        t("Kontenrahmen", "Chart of accounts"),
    ),
    (
        "books_start",
        Part::General,
        t("Bücher geführt seit", "Books kept since"),
    ),
    (
        "users",
        Part::User,
        t("Benutzer und Rollen", "Users and roles"),
    ),
    (
        "api_tokens",
        Part::User,
        t("Zugriff über die API", "API access"),
    ),
    (
        "number_ranges",
        Part::User,
        t("Nummernkreise", "Number ranges"),
    ),
    (
        "bank_accounts",
        Part::User,
        t(
            "Bankkonten in der Buchhaltung",
            "Bank accounts in the books",
        ),
    ),
    (
        "software",
        Part::Technical,
        t("Programm und Version", "Program and version"),
    ),
    ("platform", Part::Technical, t("Betrieb", "Runs as")),
    (
        "storage",
        Part::Technical,
        t("Ablage der Dateien", "File storage"),
    ),
    ("sign_in", Part::Technical, t("Anmeldung", "Sign-in")),
    ("mirror", Part::Technical, t("Spiegelung", "Mirror")),
    (
        "backup.target",
        Part::Operations,
        t("Sicherung: Ziel", "Backup: target"),
    ),
    (
        "backup.time",
        Part::Operations,
        t("Sicherung: Uhrzeit", "Backup: time"),
    ),
    (
        "backup.keep",
        Part::Operations,
        t("Sicherung: Aufbewahrung", "Backup: rotation"),
    ),
    (
        "deletions",
        Part::Operations,
        t(
            "Löschungen nach Ablauf der Fristen",
            "Deletions after retention periods",
        ),
    ),
];

/// The label of a fact or answer key as stored in a version's changes.
pub fn label(key: &str, language: Language) -> String {
    if let Some(key) = key.strip_prefix("answer.") {
        return QUESTIONS
            .iter()
            .find(|q| q.key == key)
            .map_or(key, |q| q.question.get(language))
            .to_owned();
    }
    FACTS
        .iter()
        .find(|f| f.0 == key)
        .map_or(key, |f| f.2.get(language))
        .to_owned()
}

/// What the config sets up, in the terms of the process documentation.
pub fn setup(config: &Config) -> BTreeMap<String, String> {
    let s3 = |s3: &crate::config::S3, prefix: &str| {
        let place = s3.endpoint.as_deref().unwrap_or(&s3.region);
        format!("s3://{}/{prefix} ({place})", s3.bucket)
    };
    let mut setup = BTreeMap::new();
    setup.insert(
        "storage".into(),
        config
            .storage
            .s3
            .as_ref()
            .map_or("disk".into(), |c| s3(c, "")),
    );
    setup.insert(
        "mirror".into(),
        config
            .mirror
            .webdav
            .as_ref()
            .map_or("none".into(), |w| format!("WebDAV {}", w.url)),
    );
    let mut sign_in = "passkeys".to_owned();
    if let Some(oidc) = &config.oidc {
        sign_in.push_str(&format!("; OIDC {}", oidc.issuer));
    }
    setup.insert("sign_in".into(), sign_in);
    match &config.backup {
        Some(backup) => {
            setup.insert("backup.target".into(), s3(&backup.s3, &backup.prefix));
            setup.insert(
                "backup.time".into(),
                backup.time.strftime("%H:%M").to_string(),
            );
            setup.insert(
                "backup.keep".into(),
                format!("{} / {}", backup.keep_daily, backup.keep_monthly),
            );
        }
        None => {
            setup.insert("backup.target".into(), "none".into());
        }
    }
    setup
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Content {
    pub facts: BTreeMap<String, String>,
    pub answers: BTreeMap<String, String>,
}

impl Content {
    /// The keys whose value differs, answers as `answer.<key>`.
    fn changes(&self, before: &Self) -> Vec<String> {
        let mut keys = BTreeSet::new();
        for (map, old, prefix) in [
            (&self.facts, &before.facts, ""),
            (&self.answers, &before.answers, "answer."),
        ] {
            for key in map.keys().chain(old.keys()) {
                if map.get(key) != old.get(key) {
                    keys.insert(format!("{prefix}{key}"));
                }
            }
        }
        keys.into_iter().collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Files(#[from] files::Error),
    #[error(transparent)]
    Settings(#[from] crate::settings::Error),
    #[error("cannot render the process documentation: {0}")]
    Render(#[from] render::Error),
    #[error("{0}")]
    Other(String),
}

pub async fn answers(db: &mut SqliteConnection) -> Result<BTreeMap<String, String>, sqlx::Error> {
    Ok(sqlx::query_as("SELECT key, answer FROM process_answer")
        .fetch_all(db)
        .await?
        .into_iter()
        .collect())
}

async fn facts(app: &App, db: &mut SqliteConnection) -> Result<BTreeMap<String, String>, Error> {
    let business = app.settings.business().await?;
    let mut facts = app.setup.clone();
    let address = [
        business.street.clone(),
        [business.postcode.clone(), business.city.clone()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
            .into(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(", ");
    let name = [business.name.clone(), business.legal_form.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    facts.insert(
        "business".into(),
        [name, address]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
    );
    facts.insert(
        "taxation".into(),
        serde_json::to_value(business.taxation())
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
    );
    if let Some(books) = crate::books::books(db).await? {
        facts.insert(
            "chart".into(),
            serde_json::to_value(books.chart)
                .ok()
                .and_then(|v| v.as_str().map(str::to_uppercase))
                .unwrap_or_default(),
        );
        facts.insert("books_start".into(), books.start_date);
    }
    let users: Vec<(String, String)> =
        sqlx::query_as("SELECT name, role FROM user WHERE active AND NOT anonymised ORDER BY id")
            .fetch_all(&mut *db)
            .await?;
    facts.insert(
        "users".into(),
        users
            .iter()
            .map(|(name, role)| format!("{name} ({role})"))
            .collect::<Vec<_>>()
            .join("; "),
    );
    let scopes: Vec<String> =
        sqlx::query_scalar("SELECT scopes FROM api_token WHERE revoked_at IS NULL")
            .fetch_all(&mut *db)
            .await?;
    let scopes: BTreeSet<&str> = scopes.iter().flat_map(|s| s.split(' ')).collect();
    facts.insert(
        "api_tokens".into(),
        if scopes.is_empty() {
            "none".into()
        } else {
            scopes.into_iter().collect::<Vec<_>>().join(", ")
        },
    );
    use crate::documents::Kind;
    facts.insert(
        "number_ranges".into(),
        [
            Kind::Quote,
            Kind::OrderConfirmation,
            Kind::Invoice,
            Kind::DeliveryNote,
            Kind::AcceptanceReport,
            Kind::Dunning,
        ]
        .map(|kind| kind.number_format(&business).to_owned())
        .join("; "),
    );
    let banks: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT name, iban FROM bank_account WHERE NOT archived ORDER BY id")
            .fetch_all(&mut *db)
            .await?;
    facts.insert(
        "bank_accounts".into(),
        banks
            .iter()
            .map(|(name, iban)| match iban {
                Some(iban) => format!("{name} ({iban})"),
                None => name.clone(),
            })
            .collect::<Vec<_>>()
            .join("; "),
    );
    facts.insert(
        "software".into(),
        format!(
            "Compact Company {} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("COMPACT_COMPANY_REVISION").unwrap_or("unknown revision")
        ),
    );
    facts.insert(
        "platform".into(),
        std::env::var("COMPACT_COMPANY_PLATFORM").unwrap_or_else(|_| "binary".into()),
    );
    let deletions: Vec<(i16, String, i64, bool)> = sqlx::query_as(
        "SELECT r.year, date(r.at, 'unixepoch'),
         (SELECT count(*) FROM deleted_file d WHERE d.deletion_id = r.id),
         EXISTS (SELECT 1 FROM anonymised_year a WHERE a.year = r.year AND a.at = r.at)
         FROM retention_deletion r ORDER BY r.id",
    )
    .fetch_all(&mut *db)
    .await?;
    facts.insert(
        "deletions".into(),
        if deletions.is_empty() {
            "none".into()
        } else {
            deletions
                .iter()
                .map(|(year, date, files, anonymised)| {
                    let what = if *anonymised { "anonymised" } else { "deleted" };
                    format!("{year}: {what} {date}, {files} files")
                })
                .collect::<Vec<_>>()
                .join("; ")
        },
    );
    Ok(facts)
}

pub async fn content(app: &App) -> Result<Content, Error> {
    let mut db = app.db.acquire().await?;
    Ok(Content {
        facts: facts(app, &mut db).await?,
        answers: answers(&mut db).await?,
    })
}

/// Makes a new version when a fact or answer changed since the latest one; returns its id.
pub async fn refresh(app: &App) -> Result<Option<i64>, Error> {
    let content = content(app).await?;
    let bytes = serde_json::to_vec_pretty(&content).map_err(|e| Error::Other(e.to_string()))?;
    let hash = ContentHash::of(&bytes);
    let latest: Option<(i64, String)> =
        sqlx::query_as("SELECT id, data FROM process_version ORDER BY id DESC LIMIT 1")
            .fetch_optional(&app.db)
            .await?;
    if latest.as_ref().is_some_and(|l| l.1 == hash.to_string()) {
        return Ok(None);
    }
    let (id, changes) = match &latest {
        Some((id, data)) => {
            let data: ContentHash = data.parse().map_err(|_| Error::Other("bad hash".into()))?;
            let before: Content = serde_json::from_slice(&app.files.get(data).await?)
                .map_err(|e| Error::Other(e.to_string()))?;
            (id + 1, content.changes(&before))
        }
        None => (1, Vec::new()),
    };
    let business = app.settings.business().await?;
    let valid_from = crate::api::documents::today(&business)
        .map_err(|e| Error::Other(e.message))?
        .to_string();
    let render = |language| {
        let data = document(language, id, &valid_from, &changes, &content, &business);
        async move {
            tokio::task::spawn_blocking(move || render::report(LAYOUT, &data)?.pdf())
                .await
                .map_err(|e| Error::Other(e.to_string()))?
                .map_err(Error::from)
        }
    };
    let pdf_de = app.files.put(render(Language::De).await?.into()).await?;
    let pdf_en = app.files.put(render(Language::En).await?.into()).await?;
    app.files.put(Bytes::from(bytes)).await?;
    let mut tx = app.db.begin_with("BEGIN IMMEDIATE").await?;
    let still: Option<i64> = sqlx::query_scalar("SELECT max(id) FROM process_version")
        .fetch_one(&mut *tx)
        .await?;
    if still != latest.map(|l| l.0) {
        return Ok(None);
    }
    sqlx::query(
        "INSERT INTO process_version (id, valid_from, created_at, data, pdf_de, pdf_en, changes)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&valid_from)
    .bind(now())
    .bind(hash.to_string())
    .bind(pdf_de.to_string())
    .bind(pdf_en.to_string())
    .bind(serde_json::to_string(&changes).map_err(|e| Error::Other(e.to_string()))?)
    .execute(&mut *tx)
    .await?;
    crate::mirror::queue(app, &mut tx, &format!("process:{id}")).await?;
    tx.commit().await?;
    Ok(Some(id))
}

/// The layout data of a version in one language: every text is ready to print.
fn document(
    language: Language,
    version: i64,
    valid_from: &str,
    changes: &[String],
    content: &Content,
    business: &Business,
) -> serde_json::Value {
    let tr = |text: Text| text.get(language);
    let parts: Vec<_> = PARTS
        .iter()
        .map(|&part| {
            let mut entries: Vec<serde_json::Value> = fixed(part, business.taxation())
                .into_iter()
                .map(|(label, text)| serde_json::json!({ "label": tr(label), "text": tr(text) }))
                .collect();
            entries.extend(FACTS.iter().filter(|f| f.1 == part).map(|(key, _, label)| {
                let value = content.facts.get(*key).map(|v| value(key, v, language));
                serde_json::json!({ "label": tr(*label), "text": value })
            }));
            entries.extend(QUESTIONS.iter().filter(|q| q.part == part).map(|q| {
                serde_json::json!({ "label": tr(q.question), "text": content.answers.get(q.key) })
            }));
            let (title, intro) = heading(part);
            serde_json::json!({ "title": tr(title), "intro": tr(intro), "entries": entries })
        })
        .collect();
    let de = language == Language::De;
    serde_json::json!({
        "language": if de { "de" } else { "en" },
        "title": if de { "Verfahrensdokumentation" } else { "Process documentation" },
        "business": business.name.clone().unwrap_or_default(),
        "version": version,
        "valid_from": render::date(valid_from, language),
        "software": content.facts.get("software").cloned().unwrap_or_default(),
        "preface": tr(t(
            "Diese Verfahrensdokumentation beschreibt nach den GoBD (Rz. 151 ff.), wie Belege in das Unternehmen kommen, gebucht, aufbewahrt und geschützt werden. Was Compact Company aus der Instanz kennt, ist erzeugt; das Übrige beruht auf den Antworten des Inhabers. Jede Änderung ergibt eine neue Version, die ab ihrem Datum gilt, bis die nächste sie ablöst.",
            "This process documentation describes, following the GoBD (Rz. 151 ff.), how documents reach the business and are booked, kept and protected. What Compact Company knows from the instance is generated; the rest rests on the owner's answers. Every change makes a new version, valid from its date until the next one replaces it.",
        )),
        "changes": changes.iter().map(|k| label(k, language)).collect::<Vec<_>>(),
        "labels": {
            "version": "Version",
            "valid_from": if de { "Gültig ab" } else { "Valid from" },
            "software": if de { "Programm" } else { "Program" },
            "changes": if de {
                format!("Geändert gegenüber Version {}:", version - 1)
            } else {
                format!("Changed since version {}:", version - 1)
            },
            "unanswered": if de { "Noch nicht beantwortet." } else { "Not answered yet." },
        },
        "parts": parts,
    })
}

fn heading(part: Part) -> (Text, Text) {
    match part {
        Part::General => (
            t("Allgemeine Beschreibung", "General description"),
            t(
                "Das Unternehmen, seine Buchführung und wer daran mitwirkt.",
                "The business, its bookkeeping and who takes part in it.",
            ),
        ),
        Part::User => (
            t("Anwenderdokumentation", "User documentation"),
            t(
                "Wie Rechnungen, Belege und Bankumsätze erfasst, gebucht und abgelegt werden.",
                "How invoices, receipts and bank transactions are entered, booked and filed.",
            ),
        ),
        Part::Technical => (
            t(
                "Technische Systemdokumentation",
                "Technical system documentation",
            ),
            t(
                "Das Programm, seine Datenhaltung und wie es Änderungen an Gebuchtem verhindert.",
                "The program, how it stores data and how it prevents changes to what is booked.",
            ),
        ),
        Part::Operations => (
            t("Betriebsdokumentation", "Operations documentation"),
            t(
                "Sicherung, Wiederherstellung, Spiegelung und Löschung nach Ablauf der Fristen.",
                "Backup, restore, mirror and deletion after the retention periods.",
            ),
        ),
    }
}

/// What every instance does the same way, by part.
fn fixed(part: Part, taxation: Taxation) -> Vec<(Text, Text)> {
    match part {
        Part::General => Vec::new(),
        Part::User => vec![
            (
                t("Ausgangsrechnungen", "Invoices issued"),
                t(
                    "Angebote, Rechnungen und weitere Dokumente entstehen als Entwurf und erhalten beim Ausstellen ihre fortlaufende Nummer. Vor dem Ausstellen prüft die App die Pflichtangaben nach § 14 Abs. 4 UStG. Rechnungen tragen ihre E-Rechnung (ZUGFeRD, auf Wunsch XRechnung nach EN 16931). Ein ausgestelltes Dokument ändert sich nie; korrigiert wird durch eine Stornorechnung.",
                    "Quotes, invoices and other documents start as drafts and get their sequential number when issued. Before issuing, the app checks the mandatory details of § 14 Abs. 4 UStG. Invoices carry their e-invoice (ZUGFeRD, XRechnung per EN 16931 on request). An issued document never changes; it is corrected by a cancellation invoice.",
                ),
            ),
            (
                t("Eingangsbelege", "Receipts"),
                t(
                    "Belege werden als PDF, Foto oder E-Rechnung hochgeladen; E-Rechnungen (ZUGFeRD, XRechnung CII und UBL) liest die App selbst, und das XML bleibt im Format des Eingangs. Jede Zeile erhält Konto und Steuersatz. Ein gebuchter Beleg ändert sich nie; korrigiert wird durch Stornierung.",
                    "Receipts are uploaded as PDF, photo or e-invoice; the app reads e-invoices (ZUGFeRD, XRechnung CII and UBL) itself, and the XML is kept in the format received. Each line gets an account and a VAT rate. A booked receipt never changes; it is corrected by a reversal.",
                ),
            ),
            (
                t("Buchungsregeln", "Posting rules"),
                match taxation {
                    Taxation::SmallBusiness => t(
                        "Kleinunternehmer nach § 19 UStG: Rechnungen ohne Umsatzsteuer. Eine ausgestellte Rechnung bucht Forderung an Erlös, ein Zahlungseingang Bank an Forderung. Vorsteuer wird nicht abgezogen; Belege werden brutto auf ihr Aufwandskonto gebucht.",
                        "Small business under § 19 UStG: invoices without VAT. An issued invoice books receivable to revenue, a payment bank to receivable. Input VAT is not deducted; receipts are booked gross to their expense account.",
                    ),
                    Taxation::Cash => t(
                        "Ist-Versteuerung: Eine ausgestellte Rechnung bucht Forderung an Erlös und noch nicht fällige Umsatzsteuer; mit dem Zahlungseingang wird die Umsatzsteuer fällig. Belege buchen Aufwand und Vorsteuer an Verbindlichkeit.",
                        "Cash basis: an issued invoice books receivable to revenue and VAT not yet due; the VAT becomes due when the payment arrives. Receipts book expense and input VAT to payable.",
                    ),
                    Taxation::Accrual => t(
                        "Soll-Versteuerung: Eine ausgestellte Rechnung bucht Forderung an Erlös und Umsatzsteuer des Monats der Ausstellung. Belege buchen Aufwand und Vorsteuer an Verbindlichkeit.",
                        "Accrual basis: an issued invoice books receivable to revenue and the VAT of the month it is issued in. Receipts book expense and input VAT to payable.",
                    ),
                },
            ),
        ],
        Part::Technical => vec![
            (
                t("Datenhaltung", "Data storage"),
                t(
                    "Alle Datensätze liegen in einer SQLite-Datenbank. Dateien (Belege, ausgestellte PDFs, E-Rechnungen, Kontoauszüge, Jahresarchive) werden unter ihrer SHA-256-Prüfsumme abgelegt und nie überschrieben.",
                    "Every record is in an SQLite database. Files (receipts, issued PDFs, e-invoices, bank statements, year archives) are stored under their SHA-256 checksum and never overwritten.",
                ),
            ),
            (
                t("Unveränderbarkeit", "Immutability"),
                t(
                    "Die Datenbank selbst weist Änderungen an ausgestellten Dokumenten, gebuchten Belegen, Buchungen und Kontoumsätzen zurück; korrigiert wird nur durch Gegenbuchung. Jede Änderung, die die Bücher berührt, steht mit Zeitpunkt, Benutzer und Werten vorher und nachher im Änderungsprotokoll, das sich weder ändern noch löschen lässt.",
                    "The database itself refuses changes to issued documents, booked receipts, postings and bank transactions; corrections are made only by reversal. Every change that touches the books is in the audit log with its time, user and values before and after; the log can be neither changed nor deleted.",
                ),
            ),
            (
                t("Abschluss eines Jahres", "Closing a year"),
                t(
                    "Ein abgeschlossenes Jahr nimmt keine Buchung mehr an. Sein Archiv enthält Journal, Konten, DATEV-Export, Steuerzahlen, alle Dokumente, Belege und Kontoauszüge, das Änderungsprotokoll und diese Verfahrensdokumentation, mit einer SHA-256-Prüfsumme je Datei.",
                    "A closed year takes no more postings. Its archive holds the journal, accounts, DATEV export, tax figures, every document, receipt and bank statement, the audit log and this process documentation, with a SHA-256 checksum per file.",
                ),
            ),
        ],
        Part::Operations => vec![
            (
                t("Aufbewahrung und Löschung", "Retention and deletion"),
                t(
                    "Aufbewahrt werden Bücher, Jahresarchive und diese Dokumentation zehn Jahre, Buchungsbelege, Rechnungen und Kontoauszüge acht Jahre, Handelsbriefe sechs Jahre, jeweils ab Ende des Kalenderjahres (§ 147 AO, § 14b UStG). Gelöscht wird nur nach Ablauf und nur, wenn der Inhaber bestätigt, dass keine Prüfung, kein Einspruch und keine andere offene Festsetzung das Jahr betrifft; der Datensatz bleibt mit Nummer, Datum und Beträgen, das Jahresarchiv wird ohne die gelöschten Dateien neu geschrieben und behält die ursprünglichen Prüfsummen.",
                    "Books, year archives and this documentation are kept ten years, booking documents, invoices and bank statements eight years, business letters six years, each from the end of the calendar year (§ 147 AO, § 14b UStG). Files are deleted only after their period and only when the owner confirms that no audit, objection or other open assessment concerns the year; the record stays with its number, date and amounts, and the year archive is rewritten without the deleted files, keeping the original checksums.",
                ),
            ),
            (
                t("Wiederherstellung", "Restore"),
                t(
                    "Eine Sicherung wird mit dem Befehl compact-company restore in ein leeres Datenverzeichnis zurückgespielt; danach prüft das Programm jede Datei und jedes Jahresarchiv gegen seine Prüfsumme. Der Schlüssel zum Entschlüsseln liegt nicht auf dem Server.",
                    "A backup is restored with the command compact-company restore into an empty data directory; the program then checks every file and year archive against its checksum. The key to decrypt is not kept on the server.",
                ),
            ),
        ],
    }
}

/// A fact's value as printed; most are names, dates and formats that read the same in both.
fn value(key: &str, value: &str, language: Language) -> String {
    let de = language == Language::De;
    match (key, value) {
        (_, "none") => if de { "keine" } else { "none" }.into(),
        ("taxation", "small_business") => if de {
            "Kleinunternehmer (§ 19 UStG)"
        } else {
            "Small business (§ 19 UStG)"
        }
        .into(),
        ("taxation", "cash") => if de { "Ist-Versteuerung" } else { "Cash basis" }.into(),
        ("taxation", "accrual") => if de {
            "Soll-Versteuerung"
        } else {
            "Accrual basis"
        }
        .into(),
        ("books_start", date) => render::date(date, language),
        ("platform", "nixos") => if de { "NixOS-Modul" } else { "NixOS module" }.into(),
        ("platform", "oci") => if de { "OCI-Image" } else { "OCI image" }.into(),
        ("platform", _) => if de { "Programmdatei" } else { "Program file" }.into(),
        ("storage", "disk") => if de {
            "Dateisystem des Servers"
        } else {
            "The server's disk"
        }
        .into(),
        ("backup.keep", keep) => {
            let (daily, monthly) = keep.split_once(" / ").unwrap_or((keep, ""));
            if de {
                format!(
                    "die letzte Sicherung von {daily} Tagen und von {monthly} Monaten, verschlüsselt mit age"
                )
            } else {
                format!(
                    "the newest backup of {daily} days and of {monthly} months, encrypted with age"
                )
            }
        }
        ("users", users) if de => users
            .replace("(owner)", "(Inhaber)")
            .replace("(staff)", "(Mitarbeit)")
            .replace("(accountant)", "(Buchhaltung)"),
        ("sign_in", sign_in) if de => sign_in.replace("passkeys", "Passkeys"),
        _ => value.to_owned(),
    }
}

/// Versions valid in a year, for its archive; the current one when none reaches back that far.
pub async fn versions_of_year(
    db: &mut SqliteConnection,
    year: i16,
) -> Result<Vec<(i64, String, String, String)>, sqlx::Error> {
    let versions: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT v.id, v.data, v.pdf_de, v.pdf_en FROM process_version v
         WHERE v.valid_from <= ?2 AND coalesce((SELECT min(n.valid_from) FROM process_version n
           WHERE n.id > v.id), '9999-12-31') > ?1
         ORDER BY v.id",
    )
    .bind(format!("{year}-01-01"))
    .bind(format!("{year}-12-31"))
    .fetch_all(&mut *db)
    .await?;
    if !versions.is_empty() {
        return Ok(versions);
    }
    sqlx::query_as("SELECT id, data, pdf_de, pdf_en FROM process_version ORDER BY id DESC LIMIT 1")
        .fetch_all(db)
        .await
}
