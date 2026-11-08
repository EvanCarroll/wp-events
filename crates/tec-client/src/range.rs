//! Calendar-aligned date windows for crawling: `start=week end=month` and friends.
//!
//! Boundaries are aligned to the calendar in a chosen time zone:
//!
//! | keyword | as start                  | as end (exclusive)            |
//! |---------|---------------------------|-------------------------------|
//! | `today` | today 00:00               | tomorrow 00:00                |
//! | `week`  | first day of this week    | first day of next week        |
//! | `month` | 1st of this month         | 1st of next month             |
//! | `year`  | Jan 1 this year           | Jan 1 next year               |
//! | `all`   | unbounded                 | unbounded                     |
//! | date    | that day 00:00            | the day after 00:00 (inclusive of the day) |
//! | RFC 3339 timestamp | that instant   | that instant                  |

use std::fmt;
use std::str::FromStr;

use jiff::civil::{Date, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan, Zoned};

use crate::error::{Error, Result};

/// A calendar anchor used for either end of a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    Today,
    Week,
    Month,
    Year,
    All,
    /// A whole calendar day.
    Date(Date),
    /// An exact instant.
    At(Timestamp),
}

impl FromStr for Bound {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let t = s.trim().to_ascii_lowercase();
        Ok(match t.as_str() {
            "today" | "day" => Bound::Today,
            "week" | "this-week" => Bound::Week,
            "month" | "this-month" => Bound::Month,
            "year" | "this-year" => Bound::Year,
            "all" | "any" | "unbounded" => Bound::All,
            _ => {
                if let Ok(d) = s.trim().parse::<Date>() {
                    Bound::Date(d)
                } else if let Ok(ts) = s.trim().parse::<Timestamp>() {
                    Bound::At(ts)
                } else {
                    return Err(Error::Config(format!(
                        "invalid range bound {s:?}: expected today, week, month, year, all, YYYY-MM-DD or an RFC 3339 timestamp"
                    )));
                }
            }
        })
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bound::Today => f.write_str("today"),
            Bound::Week => f.write_str("week"),
            Bound::Month => f.write_str("month"),
            Bound::Year => f.write_str("year"),
            Bound::All => f.write_str("all"),
            Bound::Date(d) => d.fmt(f),
            Bound::At(t) => t.fmt(f),
        }
    }
}

/// Which events count as "in" the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RangeMode {
    /// The event **starts** inside `[start, end)`. Matches what calendars list per day.
    #[default]
    StartsWithin,
    /// Any part of the event falls inside the window (`event.end > start && event.start < end`).
    /// Includes multi-day events that began earlier and are still running.
    Overlaps,
}

impl FromStr for RangeMode {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "starts" | "starts-within" | "start" => Ok(RangeMode::StartsWithin),
            "overlaps" | "overlap" | "during" => Ok(RangeMode::Overlaps),
            _ => Err(Error::Config(format!(
                "invalid range mode {s:?}: expected starts-within or overlaps"
            ))),
        }
    }
}

/// A crawl window, e.g. `DateRange::new(Bound::Week, Bound::Month)`.
#[derive(Debug, Clone)]
pub struct DateRange {
    pub start: Bound,
    pub end: Bound,
    pub mode: RangeMode,
    /// Zone used to align `today`/`week`/... Defaults to the system zone; for most
    /// crawls the **site's** zone is the right choice.
    pub time_zone: TimeZone,
    /// First day of the week for `week`. Defaults to Monday (WordPress's default too).
    pub week_start: Weekday,
}

impl Default for DateRange {
    fn default() -> Self {
        Self::new(Bound::All, Bound::All)
    }
}

/// A window resolved to concrete instants. `None` means unbounded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRange {
    /// Inclusive lower bound.
    pub start: Option<Zoned>,
    /// Exclusive upper bound.
    pub end: Option<Zoned>,
    pub mode: RangeMode,
}

impl DateRange {
    pub fn new(start: Bound, end: Bound) -> Self {
        Self {
            start,
            end,
            mode: RangeMode::default(),
            time_zone: TimeZone::system(),
            week_start: Weekday::Monday,
        }
    }

    pub fn mode(mut self, mode: RangeMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn time_zone(mut self, tz: TimeZone) -> Self {
        self.time_zone = tz;
        self
    }

    pub fn week_start(mut self, day: Weekday) -> Self {
        self.week_start = day;
        self
    }

    /// Resolve against the current time.
    pub fn resolve(&self) -> Result<ResolvedRange> {
        self.resolve_at(&Timestamp::now().to_zoned(self.time_zone.clone()))
    }

    /// Resolve against a given "now" (useful for tests and reproducible crawls).
    pub fn resolve_at(&self, now: &Zoned) -> Result<ResolvedRange> {
        let now = now.with_time_zone(self.time_zone.clone());
        let start = self.lower(&self.start, &now)?;
        let end = self.upper(&self.end, &now)?;
        if let (Some(s), Some(e)) = (&start, &end)
            && s >= e
        {
            return Err(Error::Config(format!(
                "empty date range: start {s} is not before end {e}"
            )));
        }
        Ok(ResolvedRange {
            start,
            end,
            mode: self.mode,
        })
    }

    fn midnight(&self, d: Date) -> Result<Zoned> {
        Ok(d.to_zoned(self.time_zone.clone())?)
    }

    fn week_first_day(&self, today: Date) -> Result<Date> {
        let back = (today.weekday().to_monday_zero_offset()
            - self.week_start.to_monday_zero_offset())
        .rem_euclid(7);
        Ok(today.checked_sub(i64::from(back).days())?)
    }

    fn lower(&self, b: &Bound, now: &Zoned) -> Result<Option<Zoned>> {
        let today = now.date();
        Ok(Some(match b {
            Bound::All => return Ok(None),
            Bound::Today => self.midnight(today)?,
            Bound::Week => self.midnight(self.week_first_day(today)?)?,
            Bound::Month => self.midnight(today.first_of_month())?,
            Bound::Year => self.midnight(today.first_of_year())?,
            Bound::Date(d) => self.midnight(*d)?,
            Bound::At(t) => t.to_zoned(self.time_zone.clone()),
        }))
    }

    fn upper(&self, b: &Bound, now: &Zoned) -> Result<Option<Zoned>> {
        let today = now.date();
        Ok(Some(match b {
            Bound::All => return Ok(None),
            Bound::Today => self.midnight(today.tomorrow()?)?,
            Bound::Week => self.midnight(self.week_first_day(today)?.checked_add(7.days())?)?,
            Bound::Month => self.midnight(today.first_of_month().checked_add(1.month())?)?,
            Bound::Year => self.midnight(today.first_of_year().checked_add(1.year())?)?,
            Bound::Date(d) => self.midnight(d.tomorrow()?)?,
            Bound::At(t) => t.to_zoned(self.time_zone.clone()),
        }))
    }
}

/// Format an instant the way PHP's `strtotime` reliably parses it, keeping the offset.
pub(crate) fn php_datetime(z: &Zoned) -> String {
    z.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Zoned {
        // Wednesday 2026-11-11 15:30 in New York.
        "2026-11-11T15:30:00-05:00[America/New_York]"
            .parse()
            .unwrap()
    }

    fn range(s: &str, e: &str) -> ResolvedRange {
        DateRange::new(s.parse().unwrap(), e.parse().unwrap())
            .time_zone(TimeZone::get("America/New_York").unwrap())
            .resolve_at(&now())
            .unwrap()
    }

    #[test]
    fn week_to_month() {
        let r = range("week", "month");
        assert_eq!(
            r.start.unwrap().to_string(),
            "2026-11-09T00:00:00-05:00[America/New_York]"
        );
        assert_eq!(
            r.end.unwrap().to_string(),
            "2026-12-01T00:00:00-05:00[America/New_York]"
        );
    }

    #[test]
    fn today_to_week_sunday_start() {
        let r = DateRange::new(Bound::Today, Bound::Week)
            .time_zone(TimeZone::get("America/New_York").unwrap())
            .week_start(Weekday::Sunday)
            .resolve_at(&now())
            .unwrap();
        assert_eq!(r.start.unwrap().date().to_string(), "2026-11-11");
        assert_eq!(r.end.unwrap().date().to_string(), "2026-11-15");
    }

    #[test]
    fn year_and_all() {
        let r = range("year", "all");
        assert_eq!(r.start.unwrap().date().to_string(), "2026-01-01");
        assert!(r.end.is_none());
        let r = range("all", "year");
        assert!(r.start.is_none());
        assert_eq!(r.end.unwrap().date().to_string(), "2027-01-01");
    }

    #[test]
    fn explicit_dates_are_inclusive_days() {
        let r = range("2026-01-05", "2026-01-05");
        assert_eq!(r.start.unwrap().date().to_string(), "2026-01-05");
        assert_eq!(r.end.unwrap().date().to_string(), "2026-01-06");
    }

    #[test]
    fn empty_range_rejected() {
        let e = DateRange::new(Bound::Year, Bound::Today)
            .time_zone(TimeZone::UTC)
            .resolve_at(&"2026-11-11T00:00:00+00:00[UTC]".parse().unwrap());
        assert!(e.is_ok());
        let e = DateRange::new("2027-01-01".parse().unwrap(), Bound::Month)
            .time_zone(TimeZone::UTC)
            .resolve_at(&"2026-11-11T00:00:00+00:00[UTC]".parse().unwrap());
        assert!(e.is_err());
    }

    #[test]
    fn php_format() {
        let z: Zoned = "2026-11-09T00:00:00-05:00[America/New_York]"
            .parse()
            .unwrap();
        assert_eq!(php_datetime(&z), "2026-11-09T00:00:00-05:00");
    }
}
