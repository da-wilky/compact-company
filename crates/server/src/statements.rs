use rust_decimal::Decimal;

use crate::xml::{Node, parse};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Format {
    Camt053,
    Csv,
}

/// The booked EUR movements of one account, as a bank statement lists them.
#[derive(Debug, Default)]
pub struct Statement {
    pub iban: Option<String>,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// ISO dates.
    pub booking_date: String,
    pub value_date: Option<String>,
    /// EUR cents, money in positive.
    pub amount: i64,
    pub counterparty: Option<String>,
    pub counterparty_iban: Option<String>,
    pub purpose: Option<String>,
    /// The end-to-end or bank reference.
    pub reference: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("neither CAMT.053 nor a CSV export with date and amount columns")]
    Unknown,
    #[error("{0} is not in EUR")]
    Currency(String),
}

/// Reads a CAMT.053 statement or a bank's CSV export.
pub fn read(bytes: &[u8]) -> Result<(Format, Statement), Error> {
    let text = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if text.trim_ascii_start().starts_with(b"<") {
        Ok((Format::Camt053, camt053(text)?))
    } else {
        Ok((Format::Csv, csv(&decode(text))?))
    }
}

fn clean(text: Option<String>) -> Option<String> {
    let text = text?.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

fn cents(text: &str) -> Option<i64> {
    let value: Decimal = text.trim().parse().ok()?;
    (value * Decimal::ONE_HUNDRED).round().try_into().ok()
}

fn camt053(xml: &[u8]) -> Result<Statement, Error> {
    let root = parse(xml).ok_or(Error::Unknown)?;
    let statement = root.at("BkToCstmrStmt/Stmt").ok_or(Error::Unknown)?;
    let mut entries = Vec::new();
    for entry in statement.all("Ntry") {
        let status = entry
            .text("Sts/Cd")
            .or_else(|| entry.text("Sts"))
            .unwrap_or_default();
        if status != "BOOK" {
            continue;
        }
        let amount = entry.child("Amt").ok_or(Error::Unknown)?;
        if let Some(currency) = amount.attribute("Ccy").filter(|c| *c != "EUR") {
            return Err(Error::Currency(currency.to_owned()));
        }
        let incoming = entry.text("CdtDbtInd").as_deref() == Some("CRDT");
        let sign = if incoming { 1 } else { -1 };
        let date = |path: &str| {
            entry.text(&format!("{path}/Dt")).or_else(|| {
                entry
                    .text(&format!("{path}/DtTm"))
                    .map(|t| t.chars().take(10).collect())
            })
        };
        let booking_date = date("BookgDt").ok_or(Error::Unknown)?;
        let value_date = date("ValDt");
        let details: Vec<&Node> = entry
            .child("NtryDtls")
            .map(|d| d.all("TxDtls").collect())
            .unwrap_or_default();
        let detail_amount = |tx: &Node| {
            tx.text("Amt")
                .or_else(|| tx.text("AmtDtls/TxAmt/Amt"))
                .and_then(|a| cents(&a))
        };
        // a batch splits into its transactions when each states its own amount
        let split = details.len() > 1 && details.iter().all(|tx| detail_amount(tx).is_some());
        let total = cents(&amount.text).ok_or(Error::Unknown)?;
        let parts: Vec<(i64, Option<&Node>)> = if split {
            details
                .iter()
                .map(|tx| (detail_amount(tx).unwrap_or_default(), Some(*tx)))
                .collect()
        } else {
            vec![(total, details.first().copied())]
        };
        for (amount, tx) in parts {
            let party = if incoming { "Dbtr" } else { "Cdtr" };
            let field = |path: &str| tx.and_then(|tx| tx.text(path));
            let purpose = tx
                .and_then(|tx| tx.child("RmtInf"))
                .map(|r| {
                    r.all("Ustrd")
                        .map(|u| u.text.trim().to_owned())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|p| !p.is_empty())
                .or_else(|| entry.text("AddtlNtryInf"));
            entries.push(Entry {
                booking_date: booking_date.clone(),
                value_date: value_date.clone(),
                amount: sign * amount,
                counterparty: clean(
                    field(&format!("RltdPties/{party}/Nm"))
                        .or_else(|| field(&format!("RltdPties/{party}/Pty/Nm"))),
                ),
                counterparty_iban: field(&format!("RltdPties/{party}Acct/Id/IBAN")),
                purpose: clean(purpose),
                reference: field("Refs/EndToEndId")
                    .filter(|r| r != "NOTPROVIDED")
                    .or_else(|| entry.text("AcctSvcrRef")),
            });
        }
    }
    Ok(Statement {
        iban: statement.text("Acct/Id/IBAN"),
        entries,
    })
}

/// UTF-8, else Windows-1252, which German banks still export.
fn decode(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => HIGH[usize::from(b - 0x80)],
            _ => char::from(b),
        })
        .collect()
}

/// The fields of one CSV record; quotes may hold the delimiter and doubled quotes.
fn record(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                fields.last_mut().expect("never empty").push('"');
            }
            '"' => quoted = !quoted,
            c if c == delimiter && !quoted => fields.push(String::new()),
            c => fields.last_mut().expect("never empty").push(c),
        }
    }
    fields.into_iter().map(|f| f.trim().to_owned()).collect()
}

/// Splits into records, keeping line breaks inside quotes.
fn records(text: &str) -> Vec<String> {
    let mut records = Vec::new();
    let mut current = String::new();
    let mut quotes = 0;
    for line in text.lines() {
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
        quotes += line.matches('"').count();
        if quotes % 2 == 0 {
            records.push(std::mem::take(&mut current));
            quotes = 0;
        }
    }
    records
}

/// Column headings of the common German bank exports, lowercased; the first found wins.
const DATE: &[&str] = &[
    "buchungstag",
    "buchungsdatum",
    "booking date",
    "datum",
    "date",
];
const VALUE_DATE: &[&str] = &["valutadatum", "valuta", "wertstellung", "value date"];
const AMOUNT: &[&str] = &[
    "betrag (€)",
    "betrag (eur)",
    "betrag",
    "umsatz in eur",
    "umsatz",
    "amount (eur)",
    "amount",
];
const COUNTERPARTY: &[&str] = &[
    "name zahlungsbeteiligter",
    "beguenstigter/zahlungspflichtiger",
    "begünstigter/zahlungspflichtiger",
    "zahlungsempfänger*in",
    "zahlungspflichtige*r",
    "auftraggeber/empfänger",
    "auftraggeber / begünstigter",
    "empfänger",
    "auftraggeber",
    "payee",
    "partner name",
];
const IBAN: &[&str] = &[
    "iban zahlungsbeteiligter",
    "kontonummer/iban",
    "iban",
    "partner iban",
];
const PURPOSE: &[&str] = &[
    "verwendungszweck",
    "vorgang/verwendungszweck",
    "payment reference",
    "buchungstext",
];
const CURRENCY: &[&str] = &["währung", "waehrung", "currency"];
const OWN_IBAN: &[&str] = &["iban auftragskonto", "auftragskonto"];

fn column(headings: &[String], names: &[&str]) -> Option<usize> {
    names
        .iter()
        .find_map(|name| headings.iter().position(|h| h == name))
}

fn csv(text: &str) -> Result<Statement, Error> {
    let records = records(text);
    // banks put a few lines about the account before the headings
    let (start, delimiter, headings) = records
        .iter()
        .enumerate()
        .take(30)
        .find_map(|(i, line)| {
            [';', ',', '\t'].into_iter().find_map(|delimiter| {
                let headings: Vec<String> = record(line, delimiter)
                    .into_iter()
                    .map(|h| h.to_lowercase())
                    .collect();
                (column(&headings, DATE).is_some() && column(&headings, AMOUNT).is_some())
                    .then_some((i, delimiter, headings))
            })
        })
        .ok_or(Error::Unknown)?;
    let at = |names| column(&headings, names);
    let (date, amount) = (at(DATE).expect("found"), at(AMOUNT).expect("found"));
    let (value_date, counterparty, iban, currency, own) = (
        at(VALUE_DATE),
        at(COUNTERPARTY),
        at(IBAN),
        at(CURRENCY),
        at(OWN_IBAN),
    );
    let purposes: Vec<usize> = PURPOSE
        .iter()
        .filter_map(|p| column(&headings, std::slice::from_ref(p)))
        .collect();
    let mut statement = Statement::default();
    for line in &records[start + 1..] {
        let fields = record(line, delimiter);
        let get = |i: Option<usize>| {
            i.and_then(|i| fields.get(i))
                .map(String::to_owned)
                .filter(|f| !f.is_empty())
        };
        let Some(booking_date) = get(Some(date)).and_then(|d| csv_date(&d)) else {
            continue;
        };
        let Some(cents) = get(Some(amount)).and_then(|a| csv_amount(&a)) else {
            continue;
        };
        if let Some(code) = get(currency).filter(|c| c != "EUR" && c != "€") {
            return Err(Error::Currency(code));
        }
        if statement.iban.is_none() {
            statement.iban = get(own).map(|i| i.replace(' ', ""));
        }
        let purpose = purposes
            .iter()
            .filter_map(|p| get(Some(*p)))
            .collect::<Vec<_>>()
            .join(" ");
        statement.entries.push(Entry {
            booking_date,
            value_date: get(value_date).and_then(|d| csv_date(&d)),
            amount: cents,
            counterparty: clean(get(counterparty)),
            counterparty_iban: get(iban).map(|i| i.replace(' ', "")),
            purpose: clean(Some(purpose)),
            reference: None,
        });
    }
    Ok(statement)
}

/// `31.12.2026`, `31.12.26` or `2026-12-31`.
fn csv_date(text: &str) -> Option<String> {
    let date = if let Some((d, rest)) = text.split_once('.') {
        let (m, y) = rest.split_once('.')?;
        let y: i16 = match y.len() {
            2 => 2000 + y.parse::<i16>().ok()?,
            4 => y.parse().ok()?,
            _ => return None,
        };
        jiff::civil::Date::new(y, m.parse().ok()?, d.parse().ok()?).ok()?
    } else {
        text.parse().ok()?
    };
    Some(date.to_string())
}

/// `-1.234,56`, `1234.56` or `1,234.56 €`; the last separator before two digits is decimal.
fn csv_amount(text: &str) -> Option<i64> {
    let plain: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '-' | '+'))
        .collect();
    let decimal = match (plain.rfind(','), plain.rfind('.')) {
        (Some(c), Some(d)) => c.max(d),
        (Some(i), None) | (None, Some(i)) if plain.len() - i <= 3 => i,
        _ => plain.len(),
    };
    let (whole, fraction) = plain.split_at(decimal);
    let whole: String = whole.chars().filter(|c| !matches!(c, ',' | '.')).collect();
    let fraction = fraction.trim_start_matches([',', '.']);
    cents(&format!(
        "{whole}.{}",
        if fraction.is_empty() { "0" } else { fraction }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_read_in_both_notations() {
        assert_eq!(csv_amount("-1.234,56"), Some(-123_456));
        assert_eq!(csv_amount("1,234.56 €"), Some(123_456));
        assert_eq!(csv_amount("12,5"), Some(1_250));
        assert_eq!(csv_amount("1.000"), Some(100_000));
        assert_eq!(csv_amount("+49"), Some(4_900));
    }

    #[test]
    fn dates_read_in_german_and_iso_notation() {
        assert_eq!(csv_date("03.02.2026").as_deref(), Some("2026-02-03"));
        assert_eq!(csv_date("03.02.26").as_deref(), Some("2026-02-03"));
        assert_eq!(csv_date("2026-02-03").as_deref(), Some("2026-02-03"));
        assert_eq!(csv_date("31.02.2026"), None);
    }
}
