use std::fmt::Write;

use crate::books::Chart;

/// What the header of a Buchungsstapel names.
pub struct Batch<'a> {
    pub consultant: u32,
    pub client: u32,
    pub chart: Chart,
    /// ISO dates within one calendar year, which is the fiscal year.
    pub from: &'a str,
    pub to: &'a str,
    /// `YYYYMMDDhhmmssfff`
    pub created: &'a str,
}

/// A booking row: `amount` EUR cents from the debited `account` to the credited `contra`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub amount: i64,
    pub account: String,
    pub contra: String,
    /// Suspends the VAT an automatic account would book, which the journal books itself.
    pub suspend_automatic: bool,
    /// ISO date.
    pub date: String,
    pub reference: String,
    pub text: String,
}

/// Seeded accounts DATEV books VAT on by itself (Automatikkonten).
pub fn automatic(chart: Chart, number: &str) -> bool {
    match chart {
        Chart::Skr03 => matches!(number, "8300" | "8400" | "3300" | "3400"),
        Chart::Skr04 => matches!(number, "4300" | "4400" | "5300" | "5400"),
    }
}

/// Splits a balanced posting's lines into rows, the largest amounts paired first.
pub fn pair(lines: &[(String, i64)]) -> Vec<(String, String, i64)> {
    let mut debits: Vec<(String, i64)> = lines.iter().filter(|l| l.1 > 0).cloned().collect();
    let mut credits: Vec<(String, i64)> = lines
        .iter()
        .filter(|l| l.1 < 0)
        .map(|(a, amount)| (a.clone(), -amount))
        .collect();
    debits.sort_by_key(|l| std::cmp::Reverse(l.1));
    credits.sort_by_key(|l| std::cmp::Reverse(l.1));
    let (mut d, mut c) = (0, 0);
    let mut rows = Vec::new();
    while d < debits.len() && c < credits.len() {
        let amount = debits[d].1.min(credits[c].1);
        rows.push((debits[d].0.clone(), credits[c].0.clone(), amount));
        debits[d].1 -= amount;
        credits[c].1 -= amount;
        if debits[d].1 == 0 {
            d += 1;
        }
        if credits[c].1 == 0 {
            c += 1;
        }
    }
    rows
}

const HEADINGS: &str = "Umsatz (ohne Soll/Haben-Kz);Soll/Haben-Kennzeichen;WKZ Umsatz;Kurs;\
    Basis-Umsatz;WKZ Basis-Umsatz;Konto;Gegenkonto (ohne BU-Schlüssel);BU-Schlüssel;Belegdatum;\
    Belegfeld 1;Belegfeld 2;Skonto;Buchungstext";

/// The DATEV-Format Buchungsstapel (EXTF, format version 700, category 21, version 13) in
/// Windows-1252, with the columns up to Buchungstext.
pub fn buchungsstapel(batch: &Batch, rows: &[Row]) -> Vec<u8> {
    let compact = |iso: &str| iso.replace('-', "");
    let length = rows
        .iter()
        .flat_map(|r| [r.account.len(), r.contra.len()])
        .max()
        .unwrap_or(4)
        .max(4);
    let chart = match batch.chart {
        Chart::Skr03 => "03",
        Chart::Skr04 => "04",
    };
    let mut out = String::new();
    write!(
        out,
        "\"EXTF\";700;21;\"Buchungsstapel\";13;{};;\"CC\";\"compact-company\";;{};{};{}0101;{};{};{};\
         {};;1;;0;\"EUR\";;;;;\"{chart}\";;;;\r\n",
        batch.created,
        batch.consultant,
        batch.client,
        &batch.from[..4],
        length,
        compact(batch.from),
        compact(batch.to),
        text(&format!("{} bis {}", batch.from, batch.to), 30),
    )
    .expect("writing to a string");
    out.push_str(HEADINGS);
    out.push_str("\r\n");
    for row in rows {
        write!(
            out,
            "{},{:02};\"S\";\"EUR\";;;;{};{};{};{}{};{};;;{}\r\n",
            row.amount / 100,
            row.amount % 100,
            row.account,
            row.contra,
            if row.suspend_automatic { "\"40\"" } else { "" },
            &row.date[8..10],
            &row.date[5..7],
            text(&reference(&row.reference), 36),
            text(&row.text, 60),
        )
        .expect("writing to a string");
    }
    windows_1252(&out)
}

/// Belegfeld 1 takes letters, digits and `$&%*+-/` only.
fn reference(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '$' | '&' | '%' | '*' | '+' | '-' | '/' => c,
            _ => '-',
        })
        .collect()
}

/// A quoted text field of at most `max` characters.
fn text(value: &str, max: usize) -> String {
    let value: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect();
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn windows_1252(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            '€' => 0x80,
            '‚' => 0x82,
            '„' => 0x84,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '–' => 0x96,
            '—' => 0x97,
            c => u8::try_from(u32::from(c))
                .ok()
                .filter(|b| !(0x80..0xa0).contains(b))
                .unwrap_or(b'?'),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_posting_splits_into_rows_that_balance() {
        let lines = |pairs: &[(&str, i64)]| -> Vec<(String, i64)> {
            pairs.iter().map(|(a, n)| ((*a).to_owned(), *n)).collect()
        };
        assert_eq!(
            pair(&lines(&[
                ("1200", 11_900),
                ("4400", -10_000),
                ("3806", -1_900)
            ])),
            vec![
                ("1200".into(), "4400".into(), 10_000),
                ("1200".into(), "3806".into(), 1_900)
            ]
        );
        let rows = pair(&lines(&[
            ("1800", 6_000),
            ("3816", 950),
            ("1200", -6_000),
            ("3806", -950),
        ]));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.iter().map(|r| r.2).sum::<i64>(), 6_950);
    }

    #[test]
    fn the_file_is_windows_1252_with_a_datev_header() {
        let batch = Batch {
            consultant: 1001,
            client: 1,
            chart: Chart::Skr04,
            from: "2026-01-01",
            to: "2026-12-31",
            created: "20261111120000000",
        };
        let rows = [Row {
            amount: 11_905,
            account: "1200".into(),
            contra: "4400".into(),
            suspend_automatic: true,
            date: "2026-03-05".into(),
            reference: "RE_2026 001".into(),
            text: "Rechnung „Bäckerei“ 5 €".into(),
        }];
        let bytes = buchungsstapel(&batch, &rows);
        let text: String = bytes.iter().map(|b| char::from(*b)).collect();
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(lines[0].split(';').count(), 31);
        assert!(lines[0].starts_with("\"EXTF\";700;21;\"Buchungsstapel\";13;20261111120000000;"));
        assert!(lines[0].contains(";1001;1;20260101;4;20260101;20261231;"));
        assert!(lines[0].ends_with(";\"04\";;;;"));
        assert_eq!(
            lines[2],
            "119,05;\"S\";\"EUR\";;;;1200;4400;\"40\";0503;\"RE-2026-001\";;;\
             \"Rechnung \u{84}B\u{e4}ckerei\u{93} 5 \u{80}\""
        );
        assert_eq!(lines[3], "");
    }
}
