use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use crate::{audit, auth::now, tax::EuerLine};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Chart {
    Skr03,
    Skr04,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum AccountType {
    Asset,
    Liability,
    Equity,
    Income,
    Expense,
}

/// A ledger account seeded when the books start, with what the posting rules book on it.
struct Seed {
    role: Option<&'static str>,
    kind: AccountType,
    skr03: &'static str,
    skr04: &'static str,
    name: &'static str,
}

const fn seed(
    role: Option<&'static str>,
    kind: AccountType,
    skr03: &'static str,
    skr04: &'static str,
    name: &'static str,
) -> Seed {
    Seed {
        role,
        kind,
        skr03,
        skr04,
        name,
    }
}

use AccountType::{Asset, Equity, Expense, Income, Liability};

/// The accounts a small service business needs, numbered and named as in DATEV's SKR 03 and
/// SKR 04 (Art.-Nr. 11174 and 11175, 2023); the owner adds any other.
const SEED: &[Seed] = &[
    seed(None, Asset, "0027", "0135", "EDV-Software"),
    seed(None, Asset, "0420", "0650", "Büroeinrichtung"),
    seed(
        None,
        Asset,
        "0480",
        "0670",
        "Geringwertige Wirtschaftsgüter",
    ),
    seed(Some("cash"), Asset, "1000", "1600", "Kasse"),
    seed(Some("bank"), Asset, "1200", "1800", "Bank"),
    seed(Some("money_transit"), Asset, "1360", "1460", "Geldtransit"),
    seed(
        Some("receivables"),
        Asset,
        "1400",
        "1200",
        "Forderungen aus Lieferungen und Leistungen",
    ),
    seed(
        Some("input_vat:7"),
        Asset,
        "1571",
        "1401",
        "Abziehbare Vorsteuer 7 %",
    ),
    seed(
        Some("input_vat:19"),
        Asset,
        "1576",
        "1406",
        "Abziehbare Vorsteuer 19 %",
    ),
    seed(None, Asset, "1590", "1370", "Durchlaufende Posten"),
    seed(
        Some("payables"),
        Liability,
        "1600",
        "3300",
        "Verbindlichkeiten aus Lieferungen und Leistungen",
    ),
    seed(
        Some("vat_pending:7"),
        Liability,
        "1761",
        "3811",
        "Umsatzsteuer nicht fällig 7 %",
    ),
    seed(
        Some("vat_pending:19"),
        Liability,
        "1766",
        "3816",
        "Umsatzsteuer nicht fällig 19 %",
    ),
    seed(Some("vat:7"), Liability, "1771", "3801", "Umsatzsteuer 7 %"),
    seed(
        Some("vat:19"),
        Liability,
        "1776",
        "3806",
        "Umsatzsteuer 19 %",
    ),
    seed(
        Some("vat_prepayments"),
        Liability,
        "1780",
        "3820",
        "Umsatzsteuer-Vorauszahlungen",
    ),
    seed(
        Some("vat_previous_year"),
        Liability,
        "1790",
        "3841",
        "Umsatzsteuer Vorjahr",
    ),
    seed(None, Equity, "0800", "2900", "Gezeichnetes Kapital"),
    seed(
        Some("private_withdrawals"),
        Equity,
        "1800",
        "2100",
        "Privatentnahmen allgemein",
    ),
    seed(
        Some("private_deposits"),
        Equity,
        "1890",
        "2180",
        "Privateinlagen",
    ),
    seed(
        Some("revenue_small_business"),
        Income,
        "8195",
        "4185",
        "Erlöse als Kleinunternehmer nach § 19 Abs. 1 UStG",
    ),
    seed(Some("revenue:7"), Income, "8300", "4300", "Erlöse 7 % USt"),
    seed(
        Some("revenue:19"),
        Income,
        "8400",
        "4400",
        "Erlöse 19 % USt",
    ),
    seed(
        Some("fx_gains"),
        Income,
        "2660",
        "4840",
        "Erträge aus der Währungsumrechnung",
    ),
    seed(
        None,
        Income,
        "2650",
        "7100",
        "Sonstige Zinsen und ähnliche Erträge",
    ),
    seed(
        Some("fx_losses"),
        Expense,
        "2150",
        "6880",
        "Aufwendungen aus der Währungsumrechnung",
    ),
    seed(
        None,
        Expense,
        "2100",
        "7300",
        "Zinsen und ähnliche Aufwendungen",
    ),
    seed(None, Expense, "3100", "5900", "Fremdleistungen"),
    seed(None, Expense, "3300", "5300", "Wareneingang 7 % Vorsteuer"),
    seed(None, Expense, "3400", "5400", "Wareneingang 19 % Vorsteuer"),
    seed(
        None,
        Expense,
        "4210",
        "6310",
        "Miete (unbewegliche Wirtschaftsgüter)",
    ),
    seed(None, Expense, "4360", "6400", "Versicherungen"),
    seed(None, Expense, "4380", "6420", "Beiträge"),
    seed(
        None,
        Expense,
        "4530",
        "6530",
        "Laufende Fahrzeug-Betriebskosten",
    ),
    seed(None, Expense, "4600", "6600", "Werbekosten"),
    seed(None, Expense, "4650", "6640", "Bewirtungskosten"),
    seed(
        None,
        Expense,
        "4673",
        "6673",
        "Reisekosten Unternehmer Fahrtkosten",
    ),
    seed(
        None,
        Expense,
        "4830",
        "6220",
        "Abschreibungen auf Sachanlagen",
    ),
    seed(
        None,
        Expense,
        "4855",
        "6260",
        "Sofortabschreibung geringwertiger Wirtschaftsgüter",
    ),
    seed(
        Some("expenses"),
        Expense,
        "4900",
        "6300",
        "Sonstige betriebliche Aufwendungen",
    ),
    seed(None, Expense, "4910", "6800", "Porto"),
    seed(None, Expense, "4920", "6805", "Telefon"),
    seed(None, Expense, "4925", "6810", "Telefax und Internetkosten"),
    seed(None, Expense, "4930", "6815", "Bürobedarf"),
    seed(
        None,
        Expense,
        "4940",
        "6820",
        "Zeitschriften, Bücher (Fachliteratur)",
    ),
    seed(None, Expense, "4945", "6821", "Fortbildungskosten"),
    seed(None, Expense, "4950", "6825", "Rechts- und Beratungskosten"),
    seed(None, Expense, "4955", "6830", "Buchführungskosten"),
    seed(
        Some("bank_fees"),
        Expense,
        "4970",
        "6855",
        "Nebenkosten des Geldverkehrs",
    ),
    seed(None, Expense, "4980", "6850", "Sonstiger Betriebsbedarf"),
];

/// Seeds the chart's accounts; run once, when the books start.
pub async fn seed_accounts(tx: &mut SqliteConnection, chart: Chart) -> Result<(), sqlx::Error> {
    for seed in SEED {
        let number = match chart {
            Chart::Skr03 => seed.skr03,
            Chart::Skr04 => seed.skr04,
        };
        sqlx::query(
            "INSERT INTO ledger_account (number, name, type, role, euer_line) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(number)
        .bind(seed.name)
        .bind(seed.kind)
        .bind(seed.role)
        .bind(EuerLine::seeded(seed.skr04, seed.kind))
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct Books {
    pub chart: Chart,
    /// ISO date of the first day booked.
    pub start_date: String,
    pub started_at: i64,
}

pub async fn books(db: &mut SqliteConnection) -> Result<Option<Books>, sqlx::Error> {
    sqlx::query_as("SELECT chart, start_date, started_at FROM books")
        .fetch_optional(db)
        .await
}

/// The account the posting rules book `role` on, such as `receivables` or `revenue:19`.
pub async fn account(db: &mut SqliteConnection, role: &str) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM ledger_account WHERE role = ?")
        .bind(role)
        .fetch_optional(db)
        .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Line {
    pub account_id: i64,
    /// EUR cents, debit positive and credit negative.
    pub amount: i64,
}

/// What a posting comes from, if not booked by hand.
#[derive(Debug, Clone, Copy, Default, Serialize, sqlx::FromRow)]
pub struct Source {
    pub document_id: Option<i64>,
    pub payment_id: Option<i64>,
    pub receipt_id: Option<i64>,
    pub receipt_payment_id: Option<i64>,
    pub bank_match_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct NewPosting {
    /// ISO date.
    pub date: String,
    pub description: String,
    pub lines: Vec<Line>,
    #[serde(flatten)]
    pub source: Source,
}

/// Books a posting in `tx`; lines on the same account are merged, and the lines must balance.
pub async fn book(
    tx: &mut SqliteConnection,
    user_id: i64,
    mut posting: NewPosting,
    reverses: Option<i64>,
) -> Result<i64, sqlx::Error> {
    let mut merged: Vec<Line> = Vec::new();
    for line in &posting.lines {
        match merged.iter_mut().find(|l| l.account_id == line.account_id) {
            Some(l) => l.amount += line.amount,
            None => merged.push(*line),
        }
    }
    merged.retain(|l| l.amount != 0);
    assert_eq!(
        merged.iter().map(|l| l.amount).sum::<i64>(),
        0,
        "a posting balances: {posting:?}"
    );
    posting.lines = merged;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO posting (date, description, reverses_id, document_id, payment_id, receipt_id,
         receipt_payment_id, bank_match_id, user_id,
         created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&posting.date)
    .bind(&posting.description)
    .bind(reverses)
    .bind(posting.source.document_id)
    .bind(posting.source.payment_id)
    .bind(posting.source.receipt_id)
    .bind(posting.source.receipt_payment_id)
    .bind(posting.source.bank_match_id)
    .bind(user_id)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    for line in &posting.lines {
        sqlx::query("INSERT INTO posting_line (posting_id, account_id, amount) VALUES (?, ?, ?)")
            .bind(id)
            .bind(line.account_id)
            .bind(line.amount)
            .execute(&mut *tx)
            .await?;
    }
    audit::record(
        tx,
        user_id,
        if reverses.is_some() {
            "posting.reverse"
        } else {
            "posting.create"
        },
        &format!("posting:{id}"),
        None,
        Some(serde_json::to_value(&posting).expect("postings serialize")),
    )
    .await?;
    Ok(id)
}

/// Books the exact opposite of posting `id` on `date`.
pub async fn reverse(
    tx: &mut SqliteConnection,
    user_id: i64,
    id: i64,
    date: &str,
) -> Result<i64, sqlx::Error> {
    let description: String = sqlx::query_scalar("SELECT description FROM posting WHERE id = ?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let source: Source = sqlx::query_as(
        "SELECT document_id, payment_id, receipt_id, receipt_payment_id, bank_match_id FROM posting
         WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let lines: Vec<(i64, i64)> =
        sqlx::query_as("SELECT account_id, amount FROM posting_line WHERE posting_id = ?")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    let posting = NewPosting {
        date: date.to_owned(),
        description: format!("Storno: {description}"),
        lines: lines
            .into_iter()
            .map(|(account_id, amount)| Line {
                account_id,
                amount: -amount,
            })
            .collect(),
        source,
    };
    book(tx, user_id, posting, Some(id)).await
}
