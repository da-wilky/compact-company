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
    Dunning,
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

/// What part of the work an invoice bills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum InvoiceType {
    /// An advance on work not yet done (Abschlagsrechnung).
    Partial,
    /// The whole work, less what was received on its partial invoices (Schlussrechnung).
    Final,
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
            Self::Dunning => "dunning",
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
            Self::Dunning => (&business.dunning_number_format, "MA-{yyyy}-{nnn}"),
            Self::Invoice | Self::Cancellation => {
                (&business.invoice_number_format, "RE-{yyyy}-{nnn}")
            }
        };
        set.as_deref().unwrap_or(default)
    }

    /// Whether the document carries prices.
    pub fn priced(self) -> bool {
        !matches!(
            self,
            Self::DeliveryNote | Self::AcceptanceReport | Self::Dunning
        )
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
    /// The invoice a cancellation invoice reverses.
    pub preceding: Option<Preceding>,
    /// The overdue invoice a dunning letter is about.
    pub dunning: Option<DunningData>,
    /// Invoices only: a partial or final invoice; none for an ordinary one.
    pub invoice_type: Option<InvoiceType>,
    /// A final invoice's paid partial invoices, deducted with their VAT (§ 14 Abs. 5 UStG).
    #[serde(default)]
    pub deductions: Vec<Deduction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Deduction {
    pub number: String,
    /// ISO date.
    pub issue_date: String,
    /// One entry per VAT rate.
    pub vat: Vec<VatData>,
    #[schema(value_type = String)]
    pub gross: Decimal,
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
    pub vat_id: Option<String>,
    /// The buyer reference of an XRechnung, required by public customers.
    pub leitweg_id: Option<String>,
    /// Whether invoices also go out as XRechnung.
    pub xrechnung: bool,
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
    /// A final invoice's deductions, gross.
    #[schema(value_type = Option<String>)]
    pub prepaid: Option<Decimal>,
    /// What a final invoice leaves to pay: the total less the deductions.
    #[schema(value_type = Option<String>)]
    pub due: Option<Decimal>,
}

impl Totals {
    /// What the invoice asks to be paid.
    pub fn payable(&self) -> Decimal {
        self.due.unwrap_or(self.gross)
    }
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
            small_business: business.small_business(),
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
    pub xml: Option<String>,
    pub valid_until: Option<String>,
    pub pricing: Option<Pricing>,
    pub source_id: Option<i64>,
    pub due_date: Option<String>,
    pub service_from: Option<String>,
    pub service_to: Option<String>,
    pub timesheet: bool,
    pub rounding: i64,
    pub dunning_level: Option<i64>,
    pub recurring_id: Option<i64>,
    pub invoice_type: Option<InvoiceType>,
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
    /// The time entries this line charges.
    #[sqlx(skip)]
    pub time_entry_ids: Vec<i64>,
    /// On a cancellation invoice: the invoice line it reverses.
    pub cancels_line_id: Option<i64>,
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
    let mut lines: Vec<LineRow> =
        sqlx::query_as("SELECT * FROM document_line WHERE document_id = ? ORDER BY position")
            .bind(document_id)
            .fetch_all(&mut *db)
            .await?;
    for line in &mut lines {
        line.time_entry_ids = sqlx::query_scalar(
            "SELECT time_entry_id FROM line_time_entry WHERE line_id = ? ORDER BY time_entry_id",
        )
        .bind(line.id)
        .fetch_all(&mut *db)
        .await?;
    }
    Ok(lines)
}

pub fn default_vat_rate(business: &Business) -> Decimal {
    if business.small_business() {
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
    // a reversal charges the VAT the invoice charged, whatever the business is now
    let vat = if let (Some(_), Some(rate)) = (line.cancels_line_id, line.vat_rate) {
        rate.0
    } else if business.small_business() {
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
        "SELECT number, name, street, postcode, city, country, email, vat_id, leitweg_id, xrechnung
         FROM customer WHERE id = ?",
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
    let (service_from, service_to, timesheet) = time_details(db, business, row).await?;
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
    let deductions: Vec<Deduction> = partials(db, row)
        .await?
        .into_iter()
        .filter_map(|(_, deduction)| deduction)
        .collect();
    let totals = priced.then(|| {
        let net: Decimal = vat.iter().map(|v| v.net).sum();
        let tax: Decimal = vat.iter().map(|v| v.tax).sum();
        let prepaid = (row.invoice_type == Some(InvoiceType::Final))
            .then(|| deductions.iter().map(|d| d.gross).sum::<Decimal>());
        Totals {
            net,
            tax,
            gross: net + tax,
            prepaid,
            due: prepaid.map(|p| net + tax - p),
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
        due_date: row.due_date.clone(),
        valid_until: row.valid_until.clone(),
        pricing: row.pricing,
        service_from,
        service_to,
        reference: reference(db, row).await?,
        timesheet,
        preceding: preceding(db, row).await?,
        dunning: dunning(db, row).await?,
        invoice_type: row.invoice_type,
        deductions,
    })
}

/// Where a partial invoice stands for its final invoice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum PartialState {
    /// Paid, so the final invoice deducts it.
    Deducted,
    /// Unpaid, so issuing the final invoice cancels it.
    Cancelled,
    /// Paid in part: settle or cancel it before issuing the final invoice.
    PartlyPaid,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PartialInvoice {
    pub id: i64,
    pub number: String,
    pub state: PartialState,
}

/// The partial invoices of a final invoice, with what it deducts for each paid one: as taken in
/// when it was issued, or for a draft the issued partial invoices of its quote, or else of its
/// project, that no final invoice took in yet. One cancelled in full is left out.
pub async fn partials(
    db: &mut SqliteConnection,
    row: &Row,
) -> Result<Vec<(PartialInvoice, Option<Deduction>)>, sqlx::Error> {
    if row.invoice_type != Some(InvoiceType::Final) {
        return Ok(Vec::new());
    }
    let ids: Vec<(i64, Option<PartialState>)> = if row.status == Status::Issued {
        sqlx::query_as(
            "SELECT partial_id, state FROM final_partial WHERE final_id = ? ORDER BY partial_id",
        )
        .bind(row.id)
        .fetch_all(&mut *db)
        .await?
    } else {
        // a quote's order confirmation counts as the quote, so partial and final invoices may
        // come from either
        sqlx::query_as(
            "WITH root (id, quote) AS (SELECT id, CASE WHEN kind = 'order_confirmation'
                 THEN coalesce(source_id, id) ELSE id END FROM document)
             SELECT d.id, NULL FROM document d WHERE d.kind = 'invoice'
             AND d.invoice_type = 'partial' AND d.status = 'issued' AND d.customer_id = ?1
             AND d.currency = ?4 AND CASE WHEN ?2 IS NOT NULL
                 THEN (SELECT quote FROM root WHERE id = d.source_id)
                     = (SELECT quote FROM root WHERE id = ?2)
                 ELSE d.project_id IS ?3 END
             AND NOT EXISTS (SELECT 1 FROM final_partial f WHERE f.partial_id = d.id)
             ORDER BY d.issue_date, d.id",
        )
        .bind(row.customer_id)
        .bind(row.source_id)
        .bind(row.project_id)
        .bind(&row.currency)
        .fetch_all(&mut *db)
        .await?
    };
    let mut partials = Vec::new();
    for (id, taken) in ids {
        let mut family: Vec<String> = sqlx::query_scalar(
            "SELECT data FROM document WHERE source_id = ? AND kind = 'cancellation'
             AND status = 'issued' ORDER BY id",
        )
        .bind(id)
        .fetch_all(&mut *db)
        .await?;
        let (number, issue_date, data): (String, String, String) =
            sqlx::query_as("SELECT number, issue_date, data FROM document WHERE id = ?")
                .bind(id)
                .fetch_one(&mut *db)
                .await?;
        family.push(data);
        let mut vat: BTreeMap<Decimal, (Decimal, Decimal)> = BTreeMap::new();
        for data in &family {
            let data: Data = serde_json::from_str(data).expect("issued data parses");
            for group in data.vat {
                let entry = vat.entry(group.rate).or_default();
                entry.0 += group.net;
                entry.1 += group.tax;
            }
        }
        let vat: Vec<VatData> = vat
            .into_iter()
            .filter(|(_, (net, tax))| !net.is_zero() || !tax.is_zero())
            .map(|(rate, (net, tax))| VatData { rate, net, tax })
            .collect();
        let gross: Decimal = vat.iter().map(|v| v.net + v.tax).sum();
        let state = match taken {
            Some(state) => state,
            None if gross.is_zero() => continue,
            None => {
                let paid: i64 = sqlx::query_scalar(
                    "SELECT coalesce(sum(amount), 0) FROM payment
                     WHERE document_id = ? AND reversed_at IS NULL",
                )
                .bind(id)
                .fetch_one(&mut *db)
                .await?;
                if amount_due(db, id).await? <= 0 {
                    PartialState::Deducted
                } else if paid == 0 {
                    PartialState::Cancelled
                } else {
                    PartialState::PartlyPaid
                }
            }
        };
        let deduction = (state == PartialState::Deducted).then(|| Deduction {
            number: number.clone(),
            issue_date,
            vat,
            gross,
        });
        partials.push((PartialInvoice { id, number, state }, deduction));
    }
    Ok(partials)
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
    if matches!(data.kind, Kind::Invoice | Kind::Cancellation) && data.customer.xrechnung {
        for key in crate::einvoice::missing(data, crate::einvoice::Flavour::Xrechnung) {
            missing.insert(key.to_owned(), FieldError::Required);
        }
    }
    if data.lines.is_empty() && data.kind != Kind::Dunning {
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

/// The sample amounts as an invoice would carry them.
fn sample_amounts(kind: Kind, language: Language, small_business: bool) -> Data {
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
            vat_id: (!small_business).then(|| "DE987654321".to_owned()),
            leitweg_id: Some("991-12345-67".to_owned()),
            xrechnung: false,
        },
        lines: if kind == Kind::Dunning {
            Vec::new()
        } else if priced {
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
            prepaid: None,
            due: None,
        }),
        due_date: matches!(kind, Kind::Invoice | Kind::Cancellation)
            .then(|| "2026-04-10".to_owned()),
        valid_until: (kind == Kind::Quote).then(|| "2026-04-26".to_owned()),
        pricing: (kind == Kind::Quote).then_some(Pricing::Effort),
        service_from: matches!(kind, Kind::Invoice | Kind::Cancellation)
            .then(|| "2026-03-02".to_owned()),
        service_to: matches!(kind, Kind::Invoice | Kind::Cancellation)
            .then(|| "2026-03-20".to_owned()),
        preceding: (kind == Kind::Cancellation).then(|| Preceding {
            number: "RE-2026-041".to_owned(),
            issue_date: "2026-03-20".to_owned(),
        }),
        dunning: (kind == Kind::Dunning).then(|| DunningData {
            level: 1,
            invoice_number: "RE-2026-041".to_owned(),
            invoice_date: "2026-03-02".to_owned(),
            invoice_due_date: Some("2026-03-16".to_owned()),
            amount_due: d("1468.49"),
            pay_until: Some("2026-04-03".to_owned()),
        }),
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
        invoice_type: None,
        deductions: Vec::new(),
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

pub fn timezone(business: &Business) -> jiff::tz::TimeZone {
    business
        .timezone
        .as_deref()
        .and_then(|tz| jiff::tz::TimeZone::get(tz).ok())
        .unwrap_or_else(|| {
            jiff::tz::TimeZone::get("Europe/Berlin").expect("the bundled database has it")
        })
}

/// The ISO date of a Unix time in the business's timezone.
pub fn day(business: &Business, secs: i64) -> String {
    jiff::Timestamp::from_second(secs)
        .expect("stored times are valid")
        .to_zoned(timezone(business))
        .date()
        .to_string()
}

/// Minutes charged for `secs` of work: whole minutes, rounded up to `rounding` minutes if set.
pub fn billed_minutes(secs: i64, rounding: i64) -> i64 {
    if rounding > 0 {
        (secs + rounding * 60 - 1) / (rounding * 60) * rounding
    } else {
        (secs + 30) / 60
    }
}

/// Hours with two decimals, as lines and timesheets show them.
pub fn hours(minutes: i64) -> Decimal {
    (Decimal::from(minutes) / Decimal::from(60))
        .round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

type Period = (Option<String>, Option<String>, Option<Vec<TimesheetEntry>>);

/// An invoice's service period from its time entries, else as set by hand, and its timesheet.
async fn time_details(
    db: &mut SqliteConnection,
    business: &Business,
    row: &Row,
) -> Result<Period, sqlx::Error> {
    if !matches!(row.kind, Kind::Invoice | Kind::Cancellation) {
        return Ok((None, None, None));
    }
    let entries: Vec<BilledEntry> = sqlx::query_as(
        "SELECT v.started_at, v.ended_at, coalesce(v.billable_secs, v.ended_at - v.started_at) AS secs,
         v.user_name, v.description, v.todo_title, v.project_name
         FROM line_time_entry x JOIN document_line l ON l.id = x.line_id
         JOIN time_entry_view v ON v.id = x.time_entry_id
         WHERE l.document_id = ? ORDER BY v.started_at",
    )
    .bind(row.id)
    .fetch_all(db)
    .await?;
    let (from, to) = match (entries.first(), entries.iter().map(|e| e.ended_at).max()) {
        (Some(first), Some(last)) => (
            Some(day(business, first.started_at)),
            Some(day(business, last - 1)),
        ),
        _ => (row.service_from.clone(), row.service_to.clone()),
    };
    let timesheet = (row.timesheet && !entries.is_empty()).then(|| {
        entries
            .iter()
            .map(|e| TimesheetEntry {
                date: day(business, e.started_at),
                user: e.user_name.clone(),
                description: match (&e.todo_title, &e.description) {
                    (Some(todo), Some(description)) => format!("{todo}: {description}"),
                    (Some(text), None) | (None, Some(text)) => text.clone(),
                    (None, None) => e.project_name.clone(),
                },
                hours: hours(billed_minutes(e.secs, row.rounding)),
            })
            .collect()
    });
    Ok((from, to, timesheet))
}

#[derive(sqlx::FromRow)]
struct BilledEntry {
    started_at: i64,
    ended_at: i64,
    secs: i64,
    user_name: String,
    description: Option<String>,
    todo_title: Option<String>,
    project_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, sqlx::FromRow)]
pub struct Preceding {
    pub number: String,
    /// ISO date.
    pub issue_date: String,
}

async fn preceding(db: &mut SqliteConnection, row: &Row) -> Result<Option<Preceding>, sqlx::Error> {
    if row.kind != Kind::Cancellation {
        return Ok(None);
    }
    sqlx::query_as("SELECT number, issue_date FROM document WHERE id = ? AND status = 'issued'")
        .bind(row.source_id)
        .fetch_optional(db)
        .await
}

/// Sample data for checking a layout before it is saved: every field filled, amounts with
/// thousands, VAT unless the business is a small business, and a cancellation invoice's amounts
/// negative.
pub fn sample(kind: Kind, language: Language, small_business: bool) -> Data {
    let mut data = sample_amounts(kind, language, small_business);
    if kind == Kind::Cancellation {
        for line in &mut data.lines {
            line.quantity = -line.quantity;
            line.net = line.net.map(|n| -n);
        }
        for group in &mut data.vat {
            group.net = -group.net;
            group.tax = -group.tax;
        }
        if let Some(totals) = &mut data.totals {
            totals.net = -totals.net;
            totals.tax = -totals.tax;
            totals.gross = -totals.gross;
        }
    }
    data
}

/// Sample data of a partial invoice, or of a final invoice deducting two paid partial invoices
/// of 30 % and 20 % of its amounts.
pub fn sample_invoice(invoice_type: InvoiceType, language: Language, small_business: bool) -> Data {
    let mut data = sample(Kind::Invoice, language, small_business);
    data.invoice_type = Some(invoice_type);
    if invoice_type == InvoiceType::Partial {
        data.timesheet = None;
        return data;
    }
    let share = |percent: i64| {
        let part = |value: Decimal| (value * Decimal::new(percent, 2)).round_dp(2);
        data.vat
            .iter()
            .map(|g| VatData {
                rate: g.rate,
                net: part(g.net),
                tax: part(g.tax),
            })
            .collect::<Vec<_>>()
    };
    data.deductions = [
        ("SAMPLE-2026-017", "2026-01-15", 30),
        ("SAMPLE-2026-029", "2026-02-16", 20),
    ]
    .into_iter()
    .map(|(number, issue_date, percent)| {
        let vat = share(percent);
        Deduction {
            number: number.to_owned(),
            issue_date: issue_date.to_owned(),
            gross: vat.iter().map(|g| g.net + g.tax).sum(),
            vat,
        }
    })
    .collect();
    let prepaid: Decimal = data.deductions.iter().map(|d| d.gross).sum();
    if let Some(totals) = &mut data.totals {
        totals.prepaid = Some(prepaid);
        totals.due = Some(totals.gross - prepaid);
    }
    data
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DunningData {
    /// 1 payment reminder, 2 dunning letter, 3 final dunning letter.
    pub level: u8,
    pub invoice_number: String,
    /// ISO dates.
    pub invoice_date: String,
    pub invoice_due_date: Option<String>,
    /// What is still owed on the invoice, in major units of its currency.
    #[schema(value_type = String)]
    pub amount_due: Decimal,
    /// ISO date by which the customer is asked to pay; a draft has none yet.
    pub pay_until: Option<String>,
}

/// What is still owed on an issued invoice, in minor units: what it asks to be paid less its
/// payments and its cancellation invoices.
pub async fn amount_due(db: &mut SqliteConnection, invoice: i64) -> Result<i64, sqlx::Error> {
    let gross = |data: &str| {
        let data: Data = serde_json::from_str(data).expect("issued data parses");
        data.totals
            .map_or(0, |t| round_minor(t.payable() * Decimal::ONE_HUNDRED))
    };
    let data: String = sqlx::query_scalar("SELECT data FROM document WHERE id = ?")
        .bind(invoice)
        .fetch_one(&mut *db)
        .await?;
    let paid: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(amount), 0) FROM payment WHERE document_id = ? AND reversed_at IS NULL",
    )
    .bind(invoice)
    .fetch_one(&mut *db)
    .await?;
    let cancellations: Vec<String> = sqlx::query_scalar(
        "SELECT data FROM document WHERE source_id = ? AND kind = 'cancellation'
         AND status = 'issued'",
    )
    .bind(invoice)
    .fetch_all(&mut *db)
    .await?;
    Ok(gross(&data) - paid + cancellations.iter().map(|c| gross(c)).sum::<i64>())
}

async fn dunning(db: &mut SqliteConnection, row: &Row) -> Result<Option<DunningData>, sqlx::Error> {
    let (Kind::Dunning, Some(invoice), Some(level)) = (row.kind, row.source_id, row.dunning_level)
    else {
        return Ok(None);
    };
    let data: String = sqlx::query_scalar("SELECT data FROM document WHERE id = ?")
        .bind(invoice)
        .fetch_one(&mut *db)
        .await?;
    let data: Data = serde_json::from_str(&data).expect("issued data parses");
    Ok(Some(DunningData {
        level: u8::try_from(level).expect("levels are 1 to 3"),
        invoice_number: data.number.unwrap_or_default(),
        invoice_date: data.issue_date.unwrap_or_default(),
        invoice_due_date: data.due_date,
        amount_due: major(amount_due(db, invoice).await?),
        pay_until: row.due_date.clone(),
    }))
}
