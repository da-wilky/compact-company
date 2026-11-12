use std::collections::{BTreeMap, BTreeSet};

use jiff::civil::Date;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;

use crate::{
    books::{self, Line, NewPosting, Source},
    rules::{Error, role, round},
};

/// The group of the Anlage AVEÜR an asset is listed in.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema, sqlx::Type,
)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum AssetKind {
    Intangible,
    Vehicle,
    Office,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Method {
    /// Equal parts over its useful life, by month in the first and last year.
    StraightLine,
    /// Written off in the year acquired (§ 6 Abs. 2 EStG).
    LowValue,
}

/// EUR cents of net cost up to which an asset may be written off at once (§ 6 Abs. 2 EStG).
pub const LOW_VALUE_LIMIT: i64 = 80_000;

#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct Asset {
    pub id: i64,
    pub name: String,
    pub kind: AssetKind,
    /// The fixed-asset account it is carried on.
    pub account_id: i64,
    pub receipt_line_id: Option<i64>,
    /// ISO date.
    pub acquired: String,
    /// EUR cents it cost, VAT included where it can't be deducted.
    pub cost: i64,
    /// EUR cents without VAT, which the low-value limit is tested on.
    pub net: i64,
    pub method: Method,
    /// Whole years, for straight-line depreciation.
    pub life_years: Option<i16>,
    /// ISO date it left the business.
    pub disposed: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

fn date(value: &str) -> Date {
    value.parse().expect("dates are stored as ISO dates")
}

impl Asset {
    fn acquired(&self) -> Date {
        date(&self.acquired)
    }

    fn disposed(&self) -> Option<Date> {
        self.disposed.as_deref().map(date)
    }

    /// Depreciation accumulated by the end of `year`, or by its disposal if earlier.
    pub fn accumulated(&self, year: i16) -> i64 {
        let acquired = self.acquired();
        if year < acquired.year() {
            return 0;
        }
        let Some(life) = self.life_years else {
            return self.cost;
        };
        let (last_year, last_month) = match self.disposed() {
            Some(d) if d.year() <= year => (d.year(), d.month()),
            _ => (year, 12),
        };
        // the month acquired and the month disposed count in full (§ 7 Abs. 1 Satz 4 EStG)
        let months = i64::from(last_year - acquired.year()) * 12
            + i64::from(last_month - acquired.month())
            + 1;
        let whole = i64::from(life) * 12;
        round(Decimal::from(self.cost) * Decimal::from(months.min(whole)) / Decimal::from(whole))
    }

    /// The depreciation of `year`.
    pub fn depreciation(&self, year: i16) -> i64 {
        self.accumulated(year) - self.accumulated(year - 1)
    }

    /// The book value written off in `year` because the asset left the business then.
    pub fn disposal(&self, year: i16) -> i64 {
        match self.disposed() {
            Some(d) if d.year() == year => self.cost - self.accumulated(year),
            _ => 0,
        }
    }

    /// The book value at the end of `year`.
    pub fn book_value(&self, year: i16) -> i64 {
        if year < self.acquired().year() || self.disposed().is_some_and(|d| d.year() <= year) {
            0
        } else {
            self.cost - self.accumulated(year)
        }
    }

    /// The years with something to book, first to last.
    pub fn years(&self) -> Vec<i16> {
        let first = self.acquired().year();
        let last = match (self.disposed(), self.life_years) {
            (Some(d), _) => d.year(),
            (None, None) => first,
            (None, Some(life)) => {
                let end = i16::from(self.acquired().month()) - 1 + life * 12 - 1;
                first + end / 12
            }
        };
        (first..=last)
            .filter(|y| self.depreciation(*y) != 0 || self.disposal(*y) != 0)
            .collect()
    }

    fn expense_role(&self) -> &'static str {
        match (self.method, self.kind) {
            (Method::LowValue, _) => "low_value_write_off",
            (Method::StraightLine, AssetKind::Intangible) => "depreciation_intangible",
            (Method::StraightLine, _) => "depreciation",
        }
    }
}

pub async fn all(db: &mut SqliteConnection) -> Result<Vec<Asset>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM asset ORDER BY acquired, id")
        .fetch_all(db)
        .await
}

pub async fn find(db: &mut SqliteConnection, id: i64) -> Result<Option<Asset>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM asset WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// The years an asset's depreciation or disposal is booked in and not reversed.
pub async fn booked_years(
    db: &mut SqliteConnection,
    id: i64,
) -> Result<BTreeSet<i16>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT CAST(substr(date, 1, 4) AS INTEGER) FROM posting p
         WHERE asset_id = ? AND reverses_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM posting r WHERE r.reverses_id = p.id)",
    )
    .bind(id)
    .fetch_all(db)
    .await
    .map(|years| years.into_iter().collect())
}

/// The first year depreciation may be booked in: the books' first year, after any closed one.
pub async fn first_open_year(db: &mut SqliteConnection) -> Result<Option<i16>, sqlx::Error> {
    let Some(books) = books::books(db).await? else {
        return Ok(None);
    };
    let closed: Option<i16> = sqlx::query_scalar("SELECT max(year) FROM closed_year")
        .fetch_one(&mut *db)
        .await?;
    let start: i16 = books.start_date[..4].parse().expect("ISO dates");
    Ok(Some(closed.map_or(start, |c| (c + 1).max(start))))
}

/// The years from `from` through `through` in which an asset has something unbooked.
pub async fn due(
    db: &mut SqliteConnection,
    asset: &Asset,
    from: i16,
    through: i16,
) -> Result<Vec<i16>, sqlx::Error> {
    let booked = booked_years(db, asset.id).await?;
    Ok(asset
        .years()
        .into_iter()
        .filter(|y| (from..=through).contains(y) && !booked.contains(y))
        .collect())
}

/// Books an asset's depreciation of `year` on 31.12., or on the day it left the business, and
/// then the book value written off by its disposal.
pub async fn book(
    tx: &mut SqliteConnection,
    user_id: i64,
    asset: &Asset,
    year: i16,
) -> Result<(), Error> {
    let source = Source {
        asset_id: Some(asset.id),
        ..Source::default()
    };
    let date = match &asset.disposed {
        Some(d) if asset.disposed().is_some_and(|d| d.year() == year) => d.clone(),
        _ => format!("{year}-12-31"),
    };
    let depreciation = asset.depreciation(year);
    if depreciation != 0 {
        let description = match asset.method {
            Method::LowValue => format!("Sofortabschreibung {}", asset.name),
            Method::StraightLine => format!("Abschreibung {year} {}", asset.name),
        };
        let expense = role(tx, asset.expense_role()).await?;
        books::book(
            tx,
            user_id,
            NewPosting {
                date: date.clone(),
                description,
                lines: vec![
                    Line {
                        account_id: expense,
                        amount: depreciation,
                    },
                    Line {
                        account_id: asset.account_id,
                        amount: -depreciation,
                    },
                ],
                source,
            },
            None,
        )
        .await?;
    }
    let disposal = asset.disposal(year);
    if disposal != 0 {
        let expense = role(tx, "asset_disposal").await?;
        books::book(
            tx,
            user_id,
            NewPosting {
                date,
                description: format!("Abgang {}", asset.name),
                lines: vec![
                    Line {
                        account_id: expense,
                        amount: disposal,
                    },
                    Line {
                        account_id: asset.account_id,
                        amount: -disposal,
                    },
                ],
                source,
            },
            None,
        )
        .await?;
    }
    Ok(())
}

/// A group of the Anlage AVEÜR for a year, in EUR cents.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct RegisterGroup {
    pub cost: i64,
    pub start: i64,
    pub additions: i64,
    pub depreciation: i64,
    pub disposals: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Register {
    pub year: i16,
    /// The groups with assets in the year; low-value assets are in none of them.
    #[schema(value_type = BTreeMap<String, RegisterGroup>)]
    pub groups: BTreeMap<AssetKind, RegisterGroup>,
    /// Low-value assets acquired in the year.
    pub low_value: Vec<Asset>,
    /// EUR cents.
    pub low_value_total: i64,
}

/// The Anlage AVEÜR of a year, from the assets' schedules.
pub fn register(assets: &[Asset], year: i16) -> Register {
    let mut groups: BTreeMap<AssetKind, RegisterGroup> = BTreeMap::new();
    let mut low_value = Vec::new();
    for asset in assets {
        let acquired = asset.acquired().year();
        if asset.method == Method::LowValue {
            if acquired == year {
                low_value.push(asset.clone());
            }
            continue;
        }
        let held = acquired <= year && asset.disposed().is_none_or(|d| d.year() >= year);
        if !held {
            continue;
        }
        let group = groups.entry(asset.kind).or_default();
        group.cost += asset.cost;
        if acquired < year {
            group.start += asset.cost - asset.accumulated(year - 1);
        } else {
            group.additions += asset.cost;
        }
        group.depreciation += asset.depreciation(year);
        group.disposals += asset.disposal(year);
        group.end += asset.book_value(year);
    }
    Register {
        year,
        groups,
        low_value_total: low_value.iter().map(|a| a.cost).sum(),
        low_value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(acquired: &str, cost: i64, life: Option<i16>, disposed: Option<&str>) -> Asset {
        Asset {
            id: 1,
            name: "Laptop".to_owned(),
            kind: AssetKind::Office,
            account_id: 1,
            receipt_line_id: None,
            acquired: acquired.to_owned(),
            cost,
            net: cost,
            method: if life.is_some() {
                Method::StraightLine
            } else {
                Method::LowValue
            },
            life_years: life,
            disposed: disposed.map(str::to_owned),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn the_first_year_counts_from_the_month_acquired_and_the_last_takes_the_rest() {
        let laptop = asset("2026-04-15", 180_000, Some(3), None);
        assert_eq!(laptop.years(), vec![2026, 2027, 2028, 2029]);
        assert_eq!(laptop.depreciation(2026), 45_000);
        assert_eq!(laptop.depreciation(2027), 60_000);
        assert_eq!(laptop.depreciation(2028), 60_000);
        assert_eq!(laptop.depreciation(2029), 15_000);
        assert_eq!(laptop.book_value(2028), 15_000);
        assert_eq!(laptop.book_value(2029), 0);
    }

    #[test]
    fn rounding_never_leaves_a_cent_behind() {
        let chair = asset("2026-01-01", 100_000, Some(3), None);
        assert_eq!(chair.years(), vec![2026, 2027, 2028]);
        let total: i64 = chair.years().iter().map(|y| chair.depreciation(*y)).sum();
        assert_eq!(total, 100_000);
        assert_eq!(chair.depreciation(2026), 33_333);
    }

    #[test]
    fn a_low_value_asset_is_written_off_in_the_year_acquired() {
        let monitor = asset("2026-11-30", 30_000, None, None);
        assert_eq!(monitor.years(), vec![2026]);
        assert_eq!(monitor.depreciation(2026), 30_000);
        assert_eq!(monitor.book_value(2026), 0);
    }

    #[test]
    fn a_disposal_depreciates_through_its_month_and_writes_off_the_rest() {
        let laptop = asset("2026-04-15", 180_000, Some(3), Some("2027-03-10"));
        assert_eq!(laptop.years(), vec![2026, 2027]);
        assert_eq!(laptop.depreciation(2027), 15_000);
        assert_eq!(laptop.disposal(2027), 120_000);
        assert_eq!(laptop.book_value(2027), 0);
        let register = register(&[laptop], 2027);
        let office = &register.groups[&AssetKind::Office];
        assert_eq!(office.start, 135_000);
        assert_eq!(office.depreciation, 15_000);
        assert_eq!(office.disposals, 120_000);
        assert_eq!(office.end, 0);
    }
}
