use jiff::{ToSpan, civil::Date};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    App,
    settings::{self, Business},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum VatPeriod {
    Monthly,
    Quarterly,
    /// Exempt from advance returns; only the annual return is due.
    Annual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Umsatzsteuer-Voranmeldung
    VatAdvance,
    /// Sondervorauszahlung for a monthly filer's permanent extension
    ExtensionPayment,
    /// Umsatzsteuererklärung
    VatAnnual,
    /// Anlage EÜR with the income tax return
    Euer,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Deadline {
    /// Stable across recomputation, such as `vat_advance:2026-10`.
    pub key: String,
    pub kind: Kind,
    /// ISO dates of the period it is for.
    pub from: String,
    pub to: String,
    /// ISO date, moved past weekends and nationwide holidays (§ 108 Abs. 3 AO).
    pub due: String,
    /// German, as the tax office names it.
    pub title: String,
}

const MONTHS: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];

/// Easter Sunday (anonymous Gregorian algorithm).
fn easter(year: i16) -> Date {
    let y = i32::from(year);
    let (a, b, c) = (y % 19, y / 100, y % 100);
    let (d, e) = (b / 4, b % 4);
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let (i, k) = (c / 4, c % 4);
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    Date::new(
        year,
        i8::try_from(month).expect("a month"),
        i8::try_from(day).expect("a day"),
    )
    .expect("Easter is a date")
}

/// Holidays in every German state.
fn holiday(date: Date) -> bool {
    let easter = easter(date.year());
    let fixed = matches!(
        (date.month(), date.day()),
        (1, 1) | (5, 1) | (10, 3) | (12, 25) | (12, 26)
    );
    fixed
        || [-2, 1, 39, 50]
            .into_iter()
            .any(|days| easter.checked_add(days.days()).ok() == Some(date))
}

/// A deadline falling on a weekend or holiday moves to the next working day.
fn working_day(mut date: Date) -> Date {
    while matches!(
        date.weekday(),
        jiff::civil::Weekday::Saturday | jiff::civil::Weekday::Sunday
    ) || holiday(date)
    {
        date = date.tomorrow().expect("dates stay in range");
    }
    date
}

fn date(year: i16, month: i8, day: i8) -> Date {
    Date::new(year, month, day).expect("a valid date")
}

/// The business's deadlines for periods in the years `first` to `last`.
pub fn deadlines(business: &Business, first: i16, last: i16) -> Vec<Deadline> {
    let small = business.small_business();
    let period = business.vat_period.unwrap_or(VatPeriod::Quarterly);
    let extension = business.vat_extension.unwrap_or(false);
    let mut all = Vec::new();
    for year in first..=last {
        if !small {
            let months: &[i8] = match period {
                VatPeriod::Monthly => &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
                VatPeriod::Quarterly => &[3, 6, 9, 12],
                VatPeriod::Annual => &[],
            };
            for &end in months {
                let start = if period == VatPeriod::Quarterly {
                    end - 2
                } else {
                    end
                };
                let to = date(year, end, 1).last_of_month();
                let due = date(year, end, 10)
                    .checked_add((1 + i32::from(extension)).months())
                    .expect("dates stay in range");
                let (key, title) = if period == VatPeriod::Quarterly {
                    let quarter = end / 3;
                    (
                        format!("vat_advance:{year}-Q{quarter}"),
                        format!("USt-Voranmeldung {quarter}. Quartal {year}"),
                    )
                } else {
                    (
                        format!("vat_advance:{year}-{end:02}"),
                        format!(
                            "USt-Voranmeldung {} {year}",
                            MONTHS[usize::try_from(end - 1).expect("a month")]
                        ),
                    )
                };
                all.push(Deadline {
                    key,
                    kind: Kind::VatAdvance,
                    from: date(year, start, 1).to_string(),
                    to: to.to_string(),
                    due: working_day(due).to_string(),
                    title,
                });
            }
            if extension && period == VatPeriod::Monthly {
                all.push(Deadline {
                    key: format!("extension:{year}"),
                    kind: Kind::ExtensionPayment,
                    from: date(year, 1, 1).to_string(),
                    to: date(year, 12, 31).to_string(),
                    due: working_day(date(year, 2, 10)).to_string(),
                    title: format!("Sondervorauszahlung {year} (Dauerfristverlängerung)"),
                });
            }
            all.push(Deadline {
                key: format!("vat_annual:{year}"),
                kind: Kind::VatAnnual,
                from: date(year, 1, 1).to_string(),
                to: date(year, 12, 31).to_string(),
                due: working_day(date(year + 1, 7, 31)).to_string(),
                title: format!("Umsatzsteuererklärung {year}"),
            });
        }
        // a UG or GmbH closes its year by balance sheet, through its tax advisor
        if business.legal_form.is_none() {
            all.push(Deadline {
                key: format!("euer:{year}"),
                kind: Kind::Euer,
                from: date(year, 1, 1).to_string(),
                to: date(year, 12, 31).to_string(),
                due: working_day(date(year + 1, 7, 31)).to_string(),
                title: format!("EÜR und Einkommensteuererklärung {year}"),
            });
        }
    }
    all.sort_by(|a, b| a.due.cmp(&b.due).then_with(|| a.key.cmp(&b.key)));
    all
}

/// An iCalendar feed of the deadlines as all-day events; `stamp` is `YYYYMMDDTHHMMSSZ`.
pub fn ics(deadlines: &[Deadline], host: &str, stamp: &str) -> String {
    let mut out = String::from(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//compact-company//Fristen//DE\r\n\
         CALSCALE:GREGORIAN\r\nX-WR-CALNAME:Steuerfristen\r\n",
    );
    for deadline in deadlines {
        let due: Date = deadline.due.parse().expect("deadlines carry dates");
        let next = due.tomorrow().expect("dates stay in range");
        let compact = |d: Date| d.strftime("%Y%m%d").to_string();
        out.push_str(&format!(
            "BEGIN:VEVENT\r\nUID:{}@{host}\r\nDTSTAMP:{stamp}\r\nDTSTART;VALUE=DATE:{}\r\n\
             DTEND;VALUE=DATE:{}\r\nSUMMARY:{}\r\nTRANSP:TRANSPARENT\r\nEND:VEVENT\r\n",
            deadline.key.replace(':', "-"),
            compact(due),
            compact(next),
            deadline.title.replace(',', "\\,").replace(';', "\\;"),
        ));
    }
    out.push_str("END:VCALENDAR\r\n");
    out
}

/// Mails the business a week and a day before each deadline; returns how many went out. A mail
/// the server refuses is tried again on the next run.
pub async fn remind(app: &App, today: Date) -> Result<usize, settings::Error> {
    let business = app.settings.business().await?;
    let (Some(mailer), Some(to)) = (&app.mailer, &business.email) else {
        return Ok(0);
    };
    let mut sent = 0;
    for deadline in deadlines(&business, today.year() - 1, today.year() + 1) {
        let due: Date = deadline.due.parse().expect("deadlines carry dates");
        let days = (due - today).get_days();
        let kind = match days {
            0..=1 => "day",
            2..=7 => "week",
            _ => continue,
        };
        let done: bool =
            sqlx::query_scalar("SELECT count(*) > 0 FROM reminder WHERE deadline = ? AND kind = ?")
                .bind(&deadline.key)
                .bind(kind)
                .fetch_one(&app.db)
                .await?;
        if done {
            continue;
        }
        let day = due.strftime("%d.%m.%Y");
        let subject = format!("Frist: {} am {day}", deadline.title);
        let body = format!(
            "Guten Tag,\n\n{} ist am {day} fällig.\nDie Zahlen dazu: {}tax\n\ncompact-company\n",
            deadline.title, app.public_url
        );
        match mailer.notify(to, &subject, &body).await {
            Ok(()) => {
                sqlx::query("INSERT INTO reminder (deadline, kind, sent_at) VALUES (?, ?, ?)")
                    .bind(&deadline.key)
                    .bind(kind)
                    .bind(crate::auth::now())
                    .execute(&app.db)
                    .await?;
                sent += 1;
            }
            Err(e) => eprintln!("reminder for {} not sent: {e}", deadline.key),
        }
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Taxation;

    #[test]
    fn deadlines_move_past_weekends_and_holidays() {
        assert_eq!(easter(2026), date(2026, 4, 5));
        assert_eq!(easter(2027), date(2027, 3, 28));
        // Saturday 10 October 2026
        assert_eq!(working_day(date(2026, 10, 10)), date(2026, 10, 12));
        // Good Friday 3 April and Easter Monday 6 April 2026 around a weekend
        assert_eq!(working_day(date(2026, 4, 3)), date(2026, 4, 7));
        assert_eq!(working_day(date(2026, 11, 10)), date(2026, 11, 10));
    }

    #[test]
    fn a_quarterly_filer_with_an_extension_and_a_small_business() {
        let business = Business {
            taxation: Some(Taxation::Accrual),
            vat_period: Some(VatPeriod::Quarterly),
            vat_extension: Some(true),
            ..Business::default()
        };
        let all = deadlines(&business, 2026, 2026);
        let due = |key: &str| all.iter().find(|d| d.key == key).unwrap().due.clone();
        // Q3 is due 10 October, a month later with the extension: Tuesday 10 November
        assert_eq!(due("vat_advance:2026-Q3"), "2026-11-10");
        // Q4: 10 January 2027 plus a month
        assert_eq!(due("vat_advance:2026-Q4"), "2027-02-10");
        // 31 July 2027 is a Saturday
        assert_eq!(due("vat_annual:2026"), "2027-08-02");
        assert_eq!(due("euer:2026"), "2027-08-02");
        assert_eq!(all.len(), 6);

        let small = Business {
            taxation: Some(Taxation::SmallBusiness),
            ..Business::default()
        };
        let keys: Vec<String> = deadlines(&small, 2026, 2026)
            .into_iter()
            .map(|d| d.key)
            .collect();
        assert_eq!(keys, ["euer:2026"]);

        let monthly = Business {
            vat_period: Some(VatPeriod::Monthly),
            legal_form: Some("UG (haftungsbeschränkt)".into()),
            ..Business::default()
        };
        let all = deadlines(&monthly, 2026, 2026);
        assert_eq!(all.len(), 13, "twelve months and the annual return");
        assert_eq!(all[0].title, "USt-Voranmeldung Januar 2026");
        assert_eq!(all[0].due, "2026-02-10");
    }
}
