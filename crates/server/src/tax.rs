use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use crate::{
    books::AccountType,
    documents::Data,
    rules::{Error, family, ids_sql, minor, rate_key, role, round},
};

/// A line of the Anlage EÜR (form 2025), by its Zeile.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema, sqlx::Type,
)]
pub enum EuerLine {
    /// Betriebseinnahmen als Kleinunternehmer
    #[serde(rename = "12")]
    #[sqlx(rename = "12")]
    SmallBusinessIncome,
    /// Umsatzsteuerpflichtige Betriebseinnahmen, netto
    #[serde(rename = "15")]
    #[sqlx(rename = "15")]
    TaxableIncome,
    /// Umsatzsteuerfreie und nicht umsatzsteuerbare Betriebseinnahmen
    #[serde(rename = "16")]
    #[sqlx(rename = "16")]
    TaxFreeIncome,
    /// Vereinnahmte Umsatzsteuer
    #[serde(rename = "17")]
    #[sqlx(rename = "17")]
    VatCollected,
    /// Vom Finanzamt erstattete Umsatzsteuer
    #[serde(rename = "18")]
    #[sqlx(rename = "18")]
    VatRefunded,
    /// Waren, Rohstoffe und Hilfsstoffe
    #[serde(rename = "27")]
    #[sqlx(rename = "27")]
    Goods,
    /// Fremdleistungen
    #[serde(rename = "29")]
    #[sqlx(rename = "29")]
    Subcontracting,
    /// Personalkosten
    #[serde(rename = "30")]
    #[sqlx(rename = "30")]
    Personnel,
    /// AfA auf immaterielle Wirtschaftsgüter
    #[serde(rename = "32")]
    #[sqlx(rename = "32")]
    IntangibleDepreciation,
    /// AfA auf bewegliche Wirtschaftsgüter
    #[serde(rename = "33")]
    #[sqlx(rename = "33")]
    Depreciation,
    /// Geringwertige Wirtschaftsgüter
    #[serde(rename = "36")]
    #[sqlx(rename = "36")]
    LowValueAssets,
    /// Restbuchwerte der ausgeschiedenen Anlagegüter
    #[serde(rename = "38")]
    #[sqlx(rename = "38")]
    DisposedBookValue,
    /// Raumkosten
    #[serde(rename = "39")]
    #[sqlx(rename = "39")]
    Rooms,
    /// Aufwendungen für doppelte Haushaltsführung
    #[serde(rename = "40")]
    #[sqlx(rename = "40")]
    DoubleHousehold,
    /// Sonstige Aufwendungen für betrieblich genutzte Grundstücke
    #[serde(rename = "41")]
    #[sqlx(rename = "41")]
    PropertyCosts,
    /// In Zeile 41 enthaltene Erhaltungsaufwendungen: counted in 41 too, and not again in the sum
    #[serde(rename = "42")]
    #[sqlx(rename = "42")]
    PropertyMaintenance,
    /// Aufwendungen für Telekommunikation
    #[serde(rename = "43")]
    #[sqlx(rename = "43")]
    Telecom,
    /// Übernachtungs- und Reisenebenkosten
    #[serde(rename = "44")]
    #[sqlx(rename = "44")]
    Travel,
    /// Fortbildungskosten
    #[serde(rename = "45")]
    #[sqlx(rename = "45")]
    Training,
    /// Kosten für Rechts- und Steuerberatung, Buchführung
    #[serde(rename = "46")]
    #[sqlx(rename = "46")]
    Advice,
    /// Miete/Leasing für bewegliche Wirtschaftsgüter (ohne Kfz)
    #[serde(rename = "47")]
    #[sqlx(rename = "47")]
    Rental,
    /// Erhaltungsaufwendungen (ohne Gebäude und Kfz)
    #[serde(rename = "48")]
    #[sqlx(rename = "48")]
    Maintenance,
    /// Beiträge, Gebühren, Abgaben und Versicherungen
    #[serde(rename = "49")]
    #[sqlx(rename = "49")]
    Fees,
    /// Laufende EDV-Kosten
    #[serde(rename = "50")]
    #[sqlx(rename = "50")]
    ItCosts,
    /// Arbeitsmittel
    #[serde(rename = "51")]
    #[sqlx(rename = "51")]
    WorkEquipment,
    /// Kosten für Abfallbeseitigung und Entsorgung
    #[serde(rename = "52")]
    #[sqlx(rename = "52")]
    Waste,
    /// Kosten für Verpackung und Transport
    #[serde(rename = "53")]
    #[sqlx(rename = "53")]
    Packaging,
    /// Werbekosten
    #[serde(rename = "54")]
    #[sqlx(rename = "54")]
    Advertising,
    /// Schuldzinsen für Anlagevermögen
    #[serde(rename = "55")]
    #[sqlx(rename = "55")]
    AssetInterest,
    /// Übrige Schuldzinsen
    #[serde(rename = "56")]
    #[sqlx(rename = "56")]
    Interest,
    /// Gezahlte Vorsteuerbeträge
    #[serde(rename = "57")]
    #[sqlx(rename = "57")]
    InputVat,
    /// An das Finanzamt gezahlte Umsatzsteuer
    #[serde(rename = "58")]
    #[sqlx(rename = "58")]
    VatPaid,
    /// Übrige unbeschränkt abziehbare Betriebsausgaben
    #[serde(rename = "60")]
    #[sqlx(rename = "60")]
    OtherExpenses,
    /// Geschenke, abziehbar: at most 50 € per recipient and year
    #[serde(rename = "62")]
    #[sqlx(rename = "62")]
    Gifts,
    /// Geschenke, nicht abziehbar
    #[serde(rename = "62_nd")]
    #[sqlx(rename = "62_nd")]
    GiftsNonDeductible,
    /// Bewirtungsaufwendungen: 70 % abziehbar, the rest under `63_nd`
    #[serde(rename = "63")]
    #[sqlx(rename = "63")]
    Entertainment,
    /// Verpflegungsmehraufwendungen, at the lump sums
    #[serde(rename = "64")]
    #[sqlx(rename = "64")]
    MealAllowance,
    /// Tagespauschale für die Tätigkeit in der häuslichen Wohnung
    #[serde(rename = "66")]
    #[sqlx(rename = "66")]
    HomeOfficeDays,
    /// Kfz: Leasingkosten
    #[serde(rename = "68")]
    #[sqlx(rename = "68")]
    VehicleLeasing,
    /// Kfz: Steuern, Versicherungen und Maut
    #[serde(rename = "69")]
    #[sqlx(rename = "69")]
    VehicleTaxes,
    /// Sonstige tatsächliche Fahrtkosten
    #[serde(rename = "70")]
    #[sqlx(rename = "70")]
    TravelCosts,
    /// Fahrtkosten für nicht zum Betriebsvermögen gehörende Fahrzeuge (Nutzungseinlage)
    #[serde(rename = "71")]
    #[sqlx(rename = "71")]
    PrivateVehicle,
    /// Fahrtkosten für Wege zwischen Wohnung und erster Betriebsstätte: taken off the expenses
    #[serde(rename = "72")]
    #[sqlx(rename = "72")]
    Commute,
    /// Mindestens abziehbare Fahrtkosten (Entfernungspauschale): added to the expenses
    #[serde(rename = "73")]
    #[sqlx(rename = "73")]
    CommuteAllowance,
    /// Nicht abziehbare Beträge, already in Zeilen 27 to 73: taken off the expenses
    #[serde(rename = "74")]
    #[sqlx(rename = "74")]
    NonDeductible,
}

impl EuerLine {
    pub fn income(self) -> bool {
        self <= Self::VatRefunded
    }

    /// How the line's amount goes into the Summe Betriebsausgaben (Zeile 75).
    fn expense_sign(self) -> i64 {
        match self {
            Self::PropertyMaintenance | Self::GiftsNonDeductible => 0,
            Self::Commute | Self::NonDeductible => -1,
            _ => 1,
        }
    }

    /// Where a new account of `kind` counts unless the owner says otherwise.
    pub fn default_for(kind: AccountType) -> Option<Self> {
        match kind {
            AccountType::Income => Some(Self::TaxableIncome),
            AccountType::Expense => Some(Self::OtherExpenses),
            AccountType::Asset | AccountType::Liability | AccountType::Equity => None,
        }
    }

    /// The line of a seeded account, by its SKR 04 number.
    pub fn seeded(skr04: &str, kind: AccountType) -> Option<Self> {
        Some(match skr04 {
            "4185" => Self::SmallBusinessIncome,
            // advances received count as income when they arrive
            "4300" | "4400" | "3260" | "3272" | "3280" => Self::TaxableIncome,
            "4840" | "7100" => Self::TaxFreeIncome,
            "3801" | "3806" | "3811" | "3816" => Self::VatCollected,
            "1401" | "1406" => Self::InputVat,
            "3820" | "3841" => Self::VatPaid,
            "5300" | "5400" => Self::Goods,
            "5900" => Self::Subcontracting,
            "6310" => Self::Rooms,
            "6200" => Self::IntangibleDepreciation,
            "6220" => Self::Depreciation,
            "6260" => Self::LowValueAssets,
            "6895" => Self::DisposedBookValue,
            "7300" => Self::Interest,
            "6640" => Self::Entertainment,
            "6610" => Self::Gifts,
            "6620" => Self::GiftsNonDeductible,
            "6674" => Self::MealAllowance,
            "6805" | "6810" => Self::Telecom,
            "6821" => Self::Training,
            "6825" | "6830" => Self::Advice,
            "6400" | "6420" => Self::Fees,
            "6800" | "6815" | "6820" => Self::WorkEquipment,
            "6600" => Self::Advertising,
            "6530" | "6673" => Self::TravelCosts,
            _ => return Self::default_for(kind).filter(|_| kind == AccountType::Expense),
        })
    }
}

/// How a part of a posting comes into the figures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A line as booked.
    Booked,
    /// A line of a payment's own posting beside the money and the receivable or payable: an
    /// exchange result, or VAT falling due under the cash basis.
    PaymentOwn,
    /// A paid invoice's or receipt's line, in the share the payment pays of it.
    Allocated,
}

#[derive(Debug, Clone, Copy)]
struct Part {
    posting: i64,
    account: i64,
    amount: i64,
    kind: Kind,
    /// Of a partial invoice, its cancellation, or a payment of either.
    advance: bool,
}

#[derive(sqlx::FromRow)]
struct PostingRow {
    id: i64,
    date: String,
    description: String,
    document_id: Option<i64>,
    payment_id: Option<i64>,
    receipt_id: Option<i64>,
    receipt_payment_id: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct AccountRow {
    id: i64,
    role: Option<String>,
    euer_line: Option<EuerLine>,
}

/// The journal of a period as the figures see it.
struct Journal {
    postings: BTreeMap<i64, (String, String)>,
    accounts: BTreeMap<i64, AccountRow>,
    /// Every line as booked.
    booked: Vec<Part>,
    /// Lines when money moves: a payment's share of what it pays instead of the invoice or
    /// receipt itself.
    realized: Vec<Part>,
}

impl Journal {
    async fn load(db: &mut SqliteConnection, from: &str, to: &str) -> Result<Self, Error> {
        let accounts: Vec<AccountRow> =
            sqlx::query_as("SELECT id, role, euer_line FROM ledger_account")
                .fetch_all(&mut *db)
                .await?;
        let accounts: BTreeMap<i64, AccountRow> = accounts.into_iter().map(|a| (a.id, a)).collect();
        let rows: Vec<PostingRow> = sqlx::query_as(
            "SELECT id, date, description, document_id, payment_id, receipt_id, receipt_payment_id
             FROM posting WHERE date >= ? AND date <= ? ORDER BY date, id",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&mut *db)
        .await?;
        let receivables = role(db, "receivables").await?;
        let payables = role(db, "payables").await?;
        let advances: BTreeSet<i64> = sqlx::query_scalar(
            "SELECT d.id FROM document d LEFT JOIN document s ON s.id = d.source_id
             WHERE d.invoice_type = 'partial' OR (d.kind = 'cancellation' AND s.invoice_type = 'partial')",
        )
        .fetch_all(&mut *db)
        .await?
        .into_iter()
        .collect();
        let mut journal = Self {
            postings: BTreeMap::new(),
            accounts,
            booked: Vec::new(),
            realized: Vec::new(),
        };
        for row in rows {
            let lines = lines_of(db, row.id).await?;
            let paid_invoice: Option<i64> = match row.payment_id {
                Some(payment) => Some(
                    sqlx::query_scalar("SELECT document_id FROM payment WHERE id = ?")
                        .bind(payment)
                        .fetch_one(&mut *db)
                        .await?,
                ),
                None => None,
            };
            let advance = row
                .document_id
                .or(paid_invoice)
                .is_some_and(|d| advances.contains(&d));
            let part = |(account, amount), kind| Part {
                posting: row.id,
                account,
                amount,
                kind,
                advance,
            };
            journal
                .booked
                .extend(lines.iter().map(|l| part(*l, Kind::Booked)));
            let paid = if let Some(payment) = row.payment_id {
                let credit = -sum_on(&lines, receivables);
                Some((invoice_lines(db, payment).await?, credit, receivables))
            } else if let Some(payment) = row.receipt_payment_id {
                let debit = sum_on(&lines, payables);
                Some((receipt_lines(db, payment, payables).await?, debit, payables))
            } else {
                None
            };
            match paid {
                Some(((paid_lines, whole), share, cleared)) => {
                    journal.realized.extend(
                        lines
                            .iter()
                            .filter(|(account, _)| *account != cleared)
                            .map(|l| part(*l, Kind::PaymentOwn)),
                    );
                    if whole != 0 {
                        journal
                            .realized
                            .extend(paid_lines.into_iter().map(|(account, amount)| {
                                let amount = round(
                                    Decimal::from(amount) * Decimal::from(share)
                                        / Decimal::from(whole),
                                );
                                part((account, amount), Kind::Allocated)
                            }));
                    }
                }
                None if row.document_id.is_some() || row.receipt_id.is_some() => {}
                None => journal
                    .realized
                    .extend(lines.iter().map(|l| part(*l, Kind::Booked))),
            }
            journal.postings.insert(row.id, (row.date, row.description));
        }
        Ok(journal)
    }

    fn role(&self, account: i64) -> Option<&str> {
        self.accounts[&account].role.as_deref()
    }
}

async fn lines_of(db: &mut SqliteConnection, posting: i64) -> Result<Vec<(i64, i64)>, sqlx::Error> {
    sqlx::query_as("SELECT account_id, amount FROM posting_line WHERE posting_id = ? ORDER BY id")
        .bind(posting)
        .fetch_all(db)
        .await
}

fn sum_on(lines: &[(i64, i64)], account: i64) -> i64 {
    lines
        .iter()
        .filter(|(a, _)| *a == account)
        .map(|(_, amount)| amount)
        .sum()
}

/// What the invoice of a payment booked beside its receivable, with the receivable it booked; an
/// invoice issued before the books started counts as issued, in its currency.
async fn invoice_lines(
    db: &mut SqliteConnection,
    payment: i64,
) -> Result<(Vec<(i64, i64)>, i64), Error> {
    let invoice: i64 = sqlx::query_scalar("SELECT document_id FROM payment WHERE id = ?")
        .bind(payment)
        .fetch_one(&mut *db)
        .await?;
    let ids = family(db, invoice).await?;
    let receivables = role(db, "receivables").await?;
    let sql = format!(
        "SELECT l.account_id, l.amount FROM posting_line l JOIN posting p ON p.id = l.posting_id
         WHERE p.document_id IN ({}) AND p.payment_id IS NULL ORDER BY l.id",
        ids_sql(&ids)
    );
    let booked: Vec<(i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *db)
        .await?;
    if !booked.is_empty() {
        let whole = sum_on(&booked, receivables);
        let lines = booked
            .into_iter()
            .filter(|(a, _)| *a != receivables)
            .collect();
        return Ok((lines, whole));
    }
    let sql = format!(
        "SELECT data FROM document WHERE id IN ({}) AND status = 'issued'",
        ids_sql(&ids)
    );
    let datas: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *db)
        .await?;
    let mut lines = Vec::new();
    let mut whole = 0;
    for data in datas {
        let data: Data = serde_json::from_str(&data).expect("issued data parses");
        for group in &data.vat {
            let revenue = if data.business.small_business {
                "revenue_small_business".to_owned()
            } else {
                format!("revenue:{}", rate_key(group.rate))
            };
            lines.push((role(db, &revenue).await?, -minor(group.net)));
            if !group.tax.is_zero() {
                let vat = format!("vat:{}", rate_key(group.rate));
                lines.push((role(db, &vat).await?, -minor(group.tax)));
            }
            whole += minor(group.net) + minor(group.tax);
        }
    }
    Ok((lines, whole))
}

/// What the receipt of a payment booked beside its payable, with the payable as a positive amount.
async fn receipt_lines(
    db: &mut SqliteConnection,
    payment: i64,
    payables: i64,
) -> Result<(Vec<(i64, i64)>, i64), Error> {
    let booked: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT l.account_id, l.amount FROM posting_line l JOIN posting p ON p.id = l.posting_id
         WHERE p.receipt_id = (SELECT receipt_id FROM receipt_payment WHERE id = ?)
           AND p.reverses_id IS NULL AND p.receipt_payment_id IS NULL ORDER BY l.id",
    )
    .bind(payment)
    .fetch_all(&mut *db)
    .await?;
    let whole = -sum_on(&booked, payables);
    let lines = booked.into_iter().filter(|(a, _)| *a != payables).collect();
    Ok((lines, whole))
}

/// A posting behind a figure and what it adds to it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Source {
    pub posting_id: i64,
    /// ISO date.
    pub date: String,
    pub description: String,
    /// EUR cents.
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Figure {
    /// The Kennzahl of the VAT return, or the Zeile of the Anlage EÜR with `_nd` for its
    /// "nicht abziehbar" column.
    pub key: String,
    /// The Kennzahl of an EÜR line.
    pub kennzahl: Option<String>,
    /// EUR cents.
    pub amount: i64,
    /// What is entered in ELSTER: whole euros, cents dropped, for a VAT base; the amount
    /// otherwise.
    pub entered: String,
    pub sources: Vec<Source>,
}

#[derive(Default)]
struct Figures(BTreeMap<String, BTreeMap<i64, i64>>);

impl Figures {
    fn add(&mut self, key: &str, posting: i64, amount: i64) {
        *self
            .0
            .entry(key.to_owned())
            .or_default()
            .entry(posting)
            .or_default() += amount;
    }

    fn total(&self, key: &str) -> i64 {
        self.0.get(key).map_or(0, |p| p.values().sum())
    }

    fn figure(&self, journal: &Journal, key: &str, whole_euros: bool) -> Figure {
        let sources = self
            .0
            .get(key)
            .into_iter()
            .flatten()
            .filter(|(_, amount)| **amount != 0)
            .map(|(posting, amount)| {
                let (date, description) = journal.postings[posting].clone();
                Source {
                    posting_id: *posting,
                    date,
                    description,
                    amount: *amount,
                }
            })
            .collect();
        let amount = self.total(key);
        let entered = if whole_euros {
            (amount / 100).to_string()
        } else {
            let sign = if amount < 0 { "-" } else { "" };
            format!("{sign}{}.{:02}", amount.abs() / 100, amount.abs() % 100)
        };
        Figure {
            key: key.to_owned(),
            kennzahl: None,
            amount,
            entered,
            sources,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Accrual,
    Cash,
}

/// The figures of a VAT advance return (Kz 81, 86, 66 and 83) for a period.
pub async fn vat_return(
    db: &mut SqliteConnection,
    basis: Basis,
    from: &str,
    to: &str,
) -> Result<Vec<Figure>, Error> {
    let journal = Journal::load(db, from, to).await?;
    // payments to and from the tax office settle VAT, they are no part of a return
    let settlements: BTreeSet<i64> = journal
        .booked
        .iter()
        .filter(|p| {
            matches!(
                journal.role(p.account),
                Some("vat_prepayments" | "vat_previous_year")
            )
        })
        .map(|p| p.posting)
        .collect();
    let mut figures = Figures::default();
    // an advance is taxed when it arrives, under the accrual basis too
    let base: Vec<&Part> = match basis {
        Basis::Accrual => journal
            .booked
            .iter()
            .filter(|p| !p.advance)
            .chain(
                journal
                    .realized
                    .iter()
                    .filter(|p| p.advance && p.kind == Kind::Allocated),
            )
            .collect(),
        Basis::Cash => journal.realized.iter().collect(),
    };
    for part in base {
        match journal.role(part.account) {
            Some("revenue:19" | "advances:19") => figures.add("81", part.posting, -part.amount),
            Some("revenue:7" | "advances:7") => figures.add("86", part.posting, -part.amount),
            _ => {}
        }
    }
    for part in journal
        .booked
        .iter()
        .filter(|p| !settlements.contains(&p.posting))
    {
        match journal.role(part.account) {
            Some("vat:19") => figures.add("81_tax", part.posting, -part.amount),
            Some("vat:7") => figures.add("86_tax", part.posting, -part.amount),
            Some(r) if r.starts_with("input_vat:") => figures.add("66", part.posting, part.amount),
            _ => {}
        }
    }
    for key in ["81_tax", "86_tax"] {
        for (posting, amount) in figures.0.get(key).cloned().unwrap_or_default() {
            figures.add("83", posting, amount);
        }
    }
    for (posting, amount) in figures.0.get("66").cloned().unwrap_or_default() {
        figures.add("83", posting, -amount);
    }
    Ok([
        ("81", true),
        ("81_tax", false),
        ("86", true),
        ("86_tax", false),
        ("66", false),
        ("83", false),
    ]
    .into_iter()
    .map(|(key, whole)| figures.figure(&journal, key, whole))
    .collect())
}

/// The 30 % of Bewirtung that may not be deducted, in Zeile 63's "nicht abziehbar" column.
const ENTERTAINMENT_NON_DEDUCTIBLE: &str = "63_nd";

/// The Kennzahl of an EÜR figure, as the form 2025 prints it.
fn kennzahl(key: &str) -> Option<&'static str> {
    Some(match key {
        "12" => "111",
        "15" => "112",
        "16" => "103",
        "17" => "140",
        "18" => "141",
        "27" => "100",
        "29" => "110",
        "30" => "120",
        "32" => "131",
        "33" => "130",
        "36" => "132",
        "38" => "135",
        "39" => "150",
        "40" => "152",
        "41" => "151",
        "42" => "153",
        "43" => "280",
        "44" => "221",
        "45" => "281",
        "46" => "194",
        "47" => "222",
        "48" => "225",
        "49" => "223",
        "50" => "228",
        "51" => "229",
        "52" => "226",
        "53" => "227",
        "54" => "224",
        "55" => "232",
        "56" => "234",
        "57" => "185",
        "58" => "186",
        "60" => "183",
        "62" => "174",
        "62_nd" => "164",
        "63" => "175",
        "63_nd" => "165",
        "64" => "171",
        "66" => "163",
        "68" => "144",
        "69" => "145",
        "70" => "146",
        "71" => "147",
        "72" => "142",
        "73" => "176",
        "74" => "139",
        _ => return None,
    })
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Euer {
    /// One entry per Zeile with an amount, in the order of the form.
    pub lines: Vec<Figure>,
    /// EUR cents.
    pub income: i64,
    pub expenses: i64,
    pub profit: i64,
}

/// The Anlage EÜR of a period: income when it arrives and expenses when they are paid
/// (§ 11 EStG); a small business reports all its income in Zeile 12.
pub async fn euer(
    db: &mut SqliteConnection,
    small_business: bool,
    from: &str,
    to: &str,
) -> Result<Euer, Error> {
    use EuerLine::*;
    let journal = Journal::load(db, from, to).await?;
    let mut figures = Figures::default();
    let mut keys = BTreeSet::new();
    for part in &journal.realized {
        let Some(line) = journal.accounts[&part.account].euer_line else {
            continue;
        };
        let tax = matches!(line, VatCollected | InputVat | VatPaid);
        if tax && part.kind == Kind::PaymentOwn {
            // VAT falling due under the cash basis moves between VAT accounts
            continue;
        }
        let line = match line {
            VatCollected if part.kind == Kind::Booked && part.amount > 0 => VatPaid,
            VatPaid if part.amount < 0 => VatRefunded,
            TaxableIncome | TaxFreeIncome if small_business => SmallBusinessIncome,
            line => line,
        };
        let amount = if line.income() {
            -part.amount
        } else {
            part.amount
        };
        let key = |line: EuerLine| {
            let key = serde_json::to_value(line).expect("lines serialize");
            key.as_str().expect("lines are strings").to_owned()
        };
        let shares = match line {
            PropertyMaintenance => vec![
                (PropertyCosts, key(PropertyCosts), amount),
                (line, key(line), amount),
            ],
            // § 4 Abs. 5 Satz 1 Nr. 2 EStG; the rounding goes to the non-deductible share
            Entertainment => {
                let deductible = round(Decimal::from(amount) * Decimal::new(7, 1));
                vec![
                    (line, key(line), deductible),
                    (
                        line,
                        ENTERTAINMENT_NON_DEDUCTIBLE.to_owned(),
                        amount - deductible,
                    ),
                ]
            }
            _ => vec![(line, key(line), amount)],
        };
        for (line, key, amount) in shares {
            figures.add(&key, part.posting, amount);
            keys.insert((line, key));
        }
    }
    let lines: Vec<Figure> = keys
        .iter()
        .map(|(_, key)| Figure {
            kennzahl: kennzahl(key).map(str::to_owned),
            ..figures.figure(&journal, key, false)
        })
        .filter(|f| f.amount != 0 || !f.sources.is_empty())
        .collect();
    let income = keys
        .iter()
        .filter(|(line, _)| line.income())
        .map(|(_, key)| figures.total(key))
        .sum::<i64>();
    let expenses = keys
        .iter()
        .filter(|(line, key)| !line.income() && key != ENTERTAINMENT_NON_DEDUCTIBLE)
        .map(|(line, key)| line.expense_sign() * figures.total(key))
        .sum::<i64>();
    Ok(Euer {
        lines,
        income,
        expenses,
        profit: income - expenses,
    })
}
