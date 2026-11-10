use std::collections::BTreeMap;

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use crate::{
    api::{customers::Language, error::FieldError},
    rates,
    settings::Business,
};

/// Version of [`Data`], the data layouts receive (ADR 0019); raise it when a field is renamed,
/// removed or retyped, not when one is added.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
#[schema(as = DocumentKind)]
pub enum Kind {
    Quote,
    OrderConfirmation,
    Invoice,
    Cancellation,
    DeliveryNote,
    AcceptanceReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
#[schema(as = DocumentStatus)]
pub enum Status {
    Draft,
    Issued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Pricing {
    /// The total is what will be invoiced.
    Fixed,
    /// The total is an estimate; the time spent is invoiced.
    Effort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Unit {
    Hour,
    Day,
    Month,
    Piece,
    LumpSum,
}

impl Kind {
    /// The number range this kind draws from; a cancellation invoice is an invoice.
    pub fn range(self) -> Kind {
        match self {
            Self::Cancellation => Self::Invoice,
            kind => kind,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Quote => "quote",
            Self::OrderConfirmation => "order_confirmation",
            Self::Invoice => "invoice",
            Self::Cancellation => "cancellation",
            Self::DeliveryNote => "delivery_note",
            Self::AcceptanceReport => "acceptance_report",
        }
    }

    pub fn number_format(self, business: &Business) -> &str {
        let (set, default) = match self.range() {
            Self::Quote => (&business.quote_number_format, "AN-{yyyy}-{nnn}"),
            Self::OrderConfirmation => (
                &business.order_confirmation_number_format,
                "AB-{yyyy}-{nnn}",
            ),
            Self::DeliveryNote => (&business.delivery_note_number_format, "LS-{yyyy}-{nnn}"),
            Self::AcceptanceReport => {
                (&business.acceptance_report_number_format, "AP-{yyyy}-{nnn}")
            }
            Self::Invoice | Self::Cancellation => {
                (&business.invoice_number_format, "RE-{yyyy}-{nnn}")
            }
        };
        set.as_deref().unwrap_or(default)
    }

    /// Whether the document carries prices.
    pub fn priced(self) -> bool {
        !matches!(self, Self::DeliveryNote | Self::AcceptanceReport)
    }
}

/// A number format: literal text with exactly one counter (`{n}`, `{nnn}`: digits padded to the
/// count of `n`) and at most one year (`{yyyy}` or `{yy}`).
pub struct NumberFormat<'a> {
    parts: Vec<Part<'a>>,
    yearly: bool,
}

enum Part<'a> {
    Text(&'a str),
    Year4,
    Year2,
    Counter(usize),
}

impl<'a> NumberFormat<'a> {
    pub fn parse(format: &'a str) -> Option<Self> {
        let mut parts = Vec::new();
        let (mut counters, mut years) = (0, 0);
        let mut rest = format;
        while let Some(start) = rest.find('{') {
            let end = start + rest[start..].find('}')?;
            if start > 0 {
                parts.push(Part::Text(&rest[..start]));
            }
            let part = match &rest[start + 1..end] {
                "yyyy" => Part::Year4,
                "yy" => Part::Year2,
                n if !n.is_empty() && n.len() <= 9 && n.chars().all(|c| c == 'n') => {
                    Part::Counter(n.len())
                }
                _ => return None,
            };
            match part {
                Part::Counter(_) => counters += 1,
                _ => years += 1,
            }
            parts.push(part);
            rest = &rest[end + 1..];
        }
        if !rest.is_empty() {
            parts.push(Part::Text(rest));
        }
        let literal_ok = parts.iter().all(|p| match p {
            Part::Text(t) => t
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_/.#".contains(c)),
            _ => true,
        });
        (counters == 1 && years <= 1 && literal_ok && format.len() <= 40).then_some(Self {
            parts,
            yearly: years == 1,
        })
    }

    pub fn yearly(&self) -> bool {
        self.yearly
    }

    pub fn render(&self, year: i16, counter: i64) -> String {
        self.parts
            .iter()
            .map(|part| match part {
                Part::Text(t) => (*t).to_owned(),
                Part::Year4 => format!("{year:04}"),
                Part::Year2 => format!("{:02}", year % 100),
                Part::Counter(width) => format!("{counter:0width$}"),
            })
            .collect()
    }
}

/// Takes the next number of `kind`'s range; call it in the transaction that issues the document.
pub async fn next_number(
    tx: &mut SqliteConnection,
    kind: Kind,
    business: &Business,
    year: i16,
) -> Result<String, sqlx::Error> {
    let format = NumberFormat::parse(kind.number_format(business))
        .expect("number formats are validated when saved");
    let range_year = if format.yearly() { year } else { 0 };
    let counter: i64 = sqlx::query_scalar(
        "INSERT INTO number_range (name, year, last) VALUES (?, ?, 1)
         ON CONFLICT (name, year) DO UPDATE SET last = last + 1 RETURNING last",
    )
    .bind(kind.range().name())
    .bind(range_year)
    .fetch_one(tx)
    .await?;
    Ok(format.render(year, counter))
}

/// Every supported currency has two decimals.
pub fn major(minor: i64) -> Decimal {
    Decimal::new(minor, 2)
}

fn round_minor(value: Decimal) -> i64 {
    value
        .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        .try_into()
        .expect("amounts stay far below i64")
}

pub fn line_net(quantity: Decimal, unit_price: i64) -> i64 {
    round_minor(quantity * Decimal::from(unit_price))
}

/// The data a layout renders, as issued or, for a draft, as it would be issued now.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(as = DocumentData)]
pub struct Data {
    pub schema: u32,
    pub kind: Kind,
    pub number: Option<String>,
    pub language: Language,
    pub currency: String,
    /// ISO date; a draft has none yet.
    pub issue_date: Option<String>,
    pub title: Option<String>,
    pub intro: Option<String>,
    pub outro: Option<String>,
    pub business: BusinessData,
    pub customer: CustomerData,
    pub lines: Vec<LineData>,
    /// One entry per VAT rate; empty for a document without prices.
    pub vat: Vec<VatData>,
    pub totals: Option<Totals>,
    /// ISO date.
    pub due_date: Option<String>,
    /// ISO date until which a quote stands.
    pub valid_until: Option<String>,
    /// Whether a quote's total is fixed or an estimate.
    pub pricing: Option<Pricing>,
    /// ISO dates of the service period.
    pub service_from: Option<String>,
    pub service_to: Option<String>,
    /// The document this one refers to, as a sentence such as `Cancels invoice RE-2026-001`.
    pub reference: Option<String>,
    /// The time entries behind an invoice, for its timesheet.
    pub timesheet: Option<Vec<TimesheetEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BusinessData {
    pub name: Option<String>,
    pub street: Option<String>,
    pub postcode: Option<String>,
    pub city: Option<String>,
    pub country: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub tax_number: Option<String>,
    pub vat_id: Option<String>,
    pub small_business: bool,
    pub legal_form: Option<String>,
    pub register_court: Option<String>,
    pub register_number: Option<String>,
    pub managing_directors: Option<String>,
    pub bank_name: Option<String>,
    pub iban: Option<String>,
    pub bic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, sqlx::FromRow)]
pub struct CustomerData {
    pub number: String,
    pub name: String,
    pub street: Option<String>,
    pub postcode: Option<String>,
    pub city: Option<String>,
    pub country: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(as = DocumentLineData)]
pub struct LineData {
    pub position: u32,
    pub description: String,
    #[schema(value_type = String)]
    pub quantity: Decimal,
    pub unit: Unit,
    /// Major units; none on an unpriced document or a draft line without a rate.
    #[schema(value_type = Option<String>)]
    pub unit_price: Option<Decimal>,
    #[schema(value_type = Option<String>)]
    pub net: Option<Decimal>,
    /// Percent.
    #[schema(value_type = Option<String>)]
    pub vat_rate: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VatData {
    #[schema(value_type = String)]
    pub rate: Decimal,
    #[schema(value_type = String)]
    pub net: Decimal,
    #[schema(value_type = String)]
    pub tax: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Totals {
    #[schema(value_type = String)]
    pub net: Decimal,
    #[schema(value_type = String)]
    pub tax: Decimal,
    #[schema(value_type = String)]
    pub gross: Decimal,
}

impl BusinessData {
    pub fn from(business: &Business) -> Self {
        let b = business.clone();
        Self {
            name: b.name,
            street: b.street,
            postcode: b.postcode,
            city: b.city,
            country: b.country.unwrap_or_else(|| "DE".to_owned()),
            email: b.email,
            phone: b.phone,
            website: b.website,
            tax_number: b.tax_number,
            vat_id: b.vat_id,
            small_business: b.small_business.unwrap_or(false),
            legal_form: b.legal_form,
            register_court: b.register_court,
            register_number: b.register_number,
            managing_directors: b.managing_directors,
            bank_name: b.bank_name,
            iban: b.iban,
            bic: b.bic,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct Row {
    pub id: i64,
    pub kind: Kind,
    pub status: Status,
    pub customer_id: i64,
    pub project_id: Option<i64>,
    pub language: Language,
    pub currency: String,
    pub title: Option<String>,
    pub intro: Option<String>,
    pub outro: Option<String>,
    pub number: Option<String>,
    pub issue_date: Option<String>,
    pub data: Option<String>,
    pub pdf: Option<String>,
    pub layout: Option<String>,
    pub valid_until: Option<String>,
    pub pricing: Option<Pricing>,
    pub source_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
#[schema(as = DocumentLine)]
pub struct LineRow {
    pub id: i64,
    pub position: i64,
    pub description: String,
    #[schema(value_type = String)]
    pub quantity: DecimalText,
    pub unit: Unit,
    pub unit_price: Option<i64>,
    #[schema(value_type = Option<String>)]
    pub vat_rate: Option<DecimalText>,
    pub project_id: Option<i64>,
    pub activity_id: Option<i64>,
}

/// A decimal stored as text, since SQLite has no decimal type.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DecimalText(pub Decimal);

impl sqlx::Type<sqlx::Sqlite> for DecimalText {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <String as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for DecimalText {
    fn decode(value: sqlx::sqlite::SqliteValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        Ok(Self(
            <&str as sqlx::Decode<sqlx::Sqlite>>::decode(value)?.parse()?,
        ))
    }
}

pub async fn lines(
    db: &mut SqliteConnection,
    document_id: i64,
) -> Result<Vec<LineRow>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM document_line WHERE document_id = ? ORDER BY position")
        .bind(document_id)
        .fetch_all(db)
        .await
}

pub fn default_vat_rate(business: &Business) -> Decimal {
    if business.small_business.unwrap_or(false) {
        Decimal::ZERO
    } else {
        business.vat_rate.unwrap_or(Decimal::from(19))
    }
}

/// A line's unit price and VAT rate as they would be issued now: a missing price is the hourly
/// rate of the line's project and activity, a missing VAT rate the business default, and a small
/// business never charges VAT.
pub async fn effective(
    db: &mut SqliteConnection,
    business: &Business,
    line: &LineRow,
) -> Result<(Option<i64>, Decimal), sqlx::Error> {
    let price = match (line.unit_price, line.project_id) {
        (Some(price), _) => Some(price),
        (None, Some(project)) => rates::resolve(&mut *db, project, line.activity_id)
            .await?
            .map(|m| m.minor),
        (None, None) => None,
    };
    let vat = if business.small_business.unwrap_or(false) {
        Decimal::ZERO
    } else {
        line.vat_rate
            .map_or_else(|| default_vat_rate(business), |v| v.0)
    };
    Ok((price, vat))
}

/// The layout data of a draft as it would be issued now.
pub async fn draft_data(
    db: &mut SqliteConnection,
    business: &Business,
    row: &Row,
) -> Result<Data, sqlx::Error> {
    let customer: CustomerData = sqlx::query_as(
        "SELECT number, name, street, postcode, city, country, email FROM customer WHERE id = ?",
    )
    .bind(row.customer_id)
    .fetch_one(&mut *db)
    .await?;
    let priced = row.kind.priced();
    let mut lines = Vec::new();
    let mut vat: BTreeMap<Decimal, i64> = BTreeMap::new();
    for (i, line) in self::lines(db, row.id).await?.into_iter().enumerate() {
        let (price, rate) = effective(db, business, &line).await?;
        let net = price.map(|p| line_net(line.quantity.0, p));
        if let (true, Some(net)) = (priced, net) {
            *vat.entry(rate).or_default() += net;
        }
        lines.push(LineData {
            position: u32::try_from(i + 1).expect("lines fit in u32"),
            description: line.description,
            quantity: line.quantity.0,
            unit: line.unit,
            unit_price: price.filter(|_| priced).map(major),
            net: net.filter(|_| priced).map(major),
            vat_rate: priced.then_some(rate),
        });
    }
    let vat: Vec<VatData> = vat
        .into_iter()
        .map(|(rate, net)| {
            let tax = round_minor(Decimal::from(net) * rate / Decimal::ONE_HUNDRED);
            VatData {
                rate,
                net: major(net),
                tax: major(tax),
            }
        })
        .collect();
    let totals = priced.then(|| {
        let net: Decimal = vat.iter().map(|v| v.net).sum();
        let tax: Decimal = vat.iter().map(|v| v.tax).sum();
        Totals {
            net,
            tax,
            gross: net + tax,
        }
    });
    Ok(Data {
        schema: SCHEMA,
        kind: row.kind,
        number: row.number.clone(),
        language: row.language,
        currency: row.currency.clone(),
        issue_date: row.issue_date.clone(),
        title: row.title.clone(),
        intro: row.intro.clone(),
        outro: row.outro.clone(),
        business: BusinessData::from(business),
        customer,
        lines,
        vat,
        totals,
        due_date: None,
        valid_until: row.valid_until.clone(),
        pricing: row.pricing,
        service_from: None,
        service_to: None,
        reference: reference(db, row).await?,
        timesheet: None,
    })
}

/// What is missing before a document may be issued, keyed by the field it concerns.
pub fn missing(data: &Data) -> BTreeMap<String, FieldError> {
    let mut missing = BTreeMap::new();
    let mut require = |key: &str, value: &Option<String>| {
        if value.is_none() {
            missing.insert(key.to_owned(), FieldError::Required);
        }
    };
    let b = &data.business;
    require("business.name", &b.name);
    require("business.street", &b.street);
    require("business.postcode", &b.postcode);
    require("business.city", &b.city);
    let c = &data.customer;
    require("customer.street", &c.street);
    require("customer.postcode", &c.postcode);
    require("customer.city", &c.city);
    if data.kind.priced() {
        if b.tax_number.is_none() && b.vat_id.is_none() {
            missing.insert("business.tax_number".to_owned(), FieldError::Required);
        }
        if b.legal_form.is_some() {
            let b = b.clone();
            for (key, value) in [
                ("business.register_court", b.register_court),
                ("business.register_number", b.register_number),
                ("business.managing_directors", b.managing_directors),
            ] {
                if value.is_none() {
                    missing.insert(key.to_owned(), FieldError::Required);
                }
            }
        }
    }
    if data.lines.is_empty() {
        missing.insert("lines".to_owned(), FieldError::Required);
    }
    for (i, line) in data.lines.iter().enumerate() {
        if data.kind.priced() && line.unit_price.is_none() {
            missing.insert(format!("lines.{i}.unit_price"), FieldError::Required);
        }
    }
    missing
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TimesheetEntry {
    /// ISO date.
    pub date: String,
    pub user: String,
    pub description: String,
    #[schema(value_type = String)]
    pub hours: Decimal,
}

/// Sample data for checking a layout before it is saved: every field filled, amounts with
/// thousands, and VAT unless the business is a small business.
pub fn sample(kind: Kind, language: Language, small_business: bool) -> Data {
    let d = |s: &str| s.parse::<Decimal>().expect("sample decimals parse");
    let lines = vec![
        LineData {
            position: 1,
            description: "Konzeption und Entwicklung".to_owned(),
            quantity: d("12.5"),
            unit: Unit::Hour,
            unit_price: Some(d("95.00")),
            net: Some(d("1187.50")),
            vat_rate: Some(if small_business { d("0") } else { d("19") }),
        },
        LineData {
            position: 2,
            description: "Hosting".to_owned(),
            quantity: d("3"),
            unit: Unit::Month,
            unit_price: Some(d("15.50")),
            net: Some(d("46.50")),
            vat_rate: Some(if small_business { d("0") } else { d("7") }),
        },
    ];
    let vat = if small_business {
        vec![VatData {
            rate: d("0"),
            net: d("1234.00"),
            tax: d("0.00"),
        }]
    } else {
        vec![
            VatData {
                rate: d("7"),
                net: d("46.50"),
                tax: d("3.26"),
            },
            VatData {
                rate: d("19"),
                net: d("1187.50"),
                tax: d("225.63"),
            },
        ]
    };
    let tax: Decimal = vat.iter().map(|v| v.tax).sum();
    let priced = kind.priced();
    Data {
        schema: SCHEMA,
        kind,
        number: Some("SAMPLE-2026-042".to_owned()),
        language,
        currency: "EUR".to_owned(),
        issue_date: Some("2026-03-27".to_owned()),
        title: None,
        intro: Some("Sample introduction.".to_owned()),
        outro: Some("Sample closing text.".to_owned()),
        business: BusinessData {
            name: Some("Beispiel UG (haftungsbeschränkt)".to_owned()),
            street: Some("Musterweg 7".to_owned()),
            postcode: Some("01067".to_owned()),
            city: Some("Dresden".to_owned()),
            country: "DE".to_owned(),
            email: Some("post@beispiel.example".to_owned()),
            phone: Some("+49 351 000000".to_owned()),
            website: Some("beispiel.example".to_owned()),
            tax_number: Some("201/234/56789".to_owned()),
            vat_id: (!small_business).then(|| "DE123456789".to_owned()),
            small_business,
            legal_form: Some("UG (haftungsbeschränkt)".to_owned()),
            register_court: Some("Amtsgericht Dresden".to_owned()),
            register_number: Some("HRB 12345".to_owned()),
            managing_directors: Some("Erika Muster".to_owned()),
            bank_name: Some("Beispielbank".to_owned()),
            iban: Some("DE89370400440532013000".to_owned()),
            bic: Some("COBADEFFXXX".to_owned()),
        },
        customer: CustomerData {
            number: "K-007".to_owned(),
            name: "Kundin GmbH".to_owned(),
            street: Some("Kundenstraße 3".to_owned()),
            postcode: Some("04109".to_owned()),
            city: Some("Leipzig".to_owned()),
            country: "DE".to_owned(),
            email: Some("rechnung@kundin.example".to_owned()),
        },
        lines: if priced {
            lines
        } else {
            lines
                .into_iter()
                .map(|l| LineData {
                    unit_price: None,
                    net: None,
                    vat_rate: None,
                    ..l
                })
                .collect()
        },
        vat: if priced { vat } else { Vec::new() },
        totals: priced.then(|| Totals {
            net: d("1234.00"),
            tax,
            gross: d("1234.00") + tax,
        }),
        due_date: matches!(kind, Kind::Invoice | Kind::Cancellation)
            .then(|| "2026-04-10".to_owned()),
        valid_until: (kind == Kind::Quote).then(|| "2026-04-26".to_owned()),
        pricing: (kind == Kind::Quote).then_some(Pricing::Effort),
        service_from: matches!(kind, Kind::Invoice | Kind::Cancellation)
            .then(|| "2026-03-02".to_owned()),
        service_to: matches!(kind, Kind::Invoice | Kind::Cancellation)
            .then(|| "2026-03-20".to_owned()),
        reference: (kind == Kind::Cancellation)
            .then(|| "Storniert Rechnung RE-2026-041".to_owned()),
        timesheet: (kind == Kind::Invoice).then(|| {
            vec![TimesheetEntry {
                date: "2026-03-02".to_owned(),
                user: "Erika Muster".to_owned(),
                description: "Konzeption".to_owned(),
                hours: d("4.25"),
            }]
        }),
    }
}

/// The sentence naming the document this one follows from, in the document's language.
async fn reference(db: &mut SqliteConnection, row: &Row) -> Result<Option<String>, sqlx::Error> {
    let Some(source_id) = row.source_id else {
        return Ok(None);
    };
    let (kind, number): (Kind, Option<String>) =
        sqlx::query_as("SELECT kind, number FROM document WHERE id = ?")
            .bind(source_id)
            .fetch_one(db)
            .await?;
    let number = number.unwrap_or_default();
    let de = row.language == Language::De;
    Ok(Some(match (row.kind, kind) {
        (Kind::Cancellation, _) if de => format!("Storniert Rechnung {number}"),
        (Kind::Cancellation, _) => format!("Cancels invoice {number}"),
        (_, Kind::Quote) if de => format!("Zu Angebot {number}"),
        (_, Kind::Quote) => format!("For quote {number}"),
        (_, Kind::OrderConfirmation) if de => format!("Zu Auftragsbestätigung {number}"),
        (_, Kind::OrderConfirmation) => format!("For order confirmation {number}"),
        (_, _) if de => format!("Zu Rechnung {number}"),
        (_, _) => format!("For invoice {number}"),
    }))
}
