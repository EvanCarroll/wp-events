//! Typed RFC 5545 recurrence rules, and inference of a rule from observed occurrences.
//!
//! Neither API exposes the recurrence rule an editor configured, only the generated
//! occurrences. [`infer`] recovers the simplest rule that explains them: DAILY, WEEKLY (interval
//! 1–4, any weekdays), MONTHLY by day of month or by nth/last weekday, or YEARLY. Dates the rule
//! produces but the site lacks become `EXDATE`s; dates the site has but the rule lacks become
//! `RDATE`s.

use std::fmt;

use jiff::civil::{Date, Time, Weekday};
use jiff::{SignedDuration, Timestamp, ToSpan, Zoned};
use serde::{Serialize, Serializer};

/// `FREQ`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Frequency {
    fn as_str(self) -> &'static str {
        match self {
            Frequency::Daily => "DAILY",
            Frequency::Weekly => "WEEKLY",
            Frequency::Monthly => "MONTHLY",
            Frequency::Yearly => "YEARLY",
        }
    }
}

/// A `BYDAY` entry: a weekday, optionally the nth (`2TH`) or nth-from-last (`-1FR`) in the month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WeekdayNum {
    pub nth: Option<i8>,
    pub weekday: Weekday,
}

fn weekday_code(w: Weekday) -> &'static str {
    match w {
        Weekday::Monday => "MO",
        Weekday::Tuesday => "TU",
        Weekday::Wednesday => "WE",
        Weekday::Thursday => "TH",
        Weekday::Friday => "FR",
        Weekday::Saturday => "SA",
        Weekday::Sunday => "SU",
    }
}

impl fmt::Display for WeekdayNum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(n) = self.nth {
            write!(f, "{n}")?;
        }
        f.write_str(weekday_code(self.weekday))
    }
}

/// A recurrence rule (the subset this crate infers). `Display` gives the RFC 5545 value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RRule {
    pub freq: Frequency,
    /// `INTERVAL`, ≥ 1.
    pub interval: u32,
    pub by_day: Vec<WeekdayNum>,
    pub by_month_day: Vec<i8>,
    /// `UNTIL` (inclusive), as an instant.
    pub until: Option<Timestamp>,
}

impl fmt::Display for RRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FREQ={}", self.freq.as_str())?;
        if self.interval > 1 {
            write!(f, ";INTERVAL={}", self.interval)?;
        }
        if !self.by_day.is_empty() {
            let days: Vec<String> = self.by_day.iter().map(ToString::to_string).collect();
            write!(f, ";BYDAY={}", days.join(","))?;
        }
        if !self.by_month_day.is_empty() {
            let days: Vec<String> = self.by_month_day.iter().map(ToString::to_string).collect();
            write!(f, ";BYMONTHDAY={}", days.join(","))?;
        }
        if let Some(u) = self.until {
            write!(f, ";UNTIL={}", u.strftime("%Y%m%dT%H%M%SZ"))?;
        }
        Ok(())
    }
}

impl Serialize for RRule {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl RRule {
    /// Does the rule, anchored at `dtstart`, produce `d`? (Ignores `UNTIL`.)
    pub fn matches(&self, dtstart: Date, d: Date) -> bool {
        if d < dtstart {
            return false;
        }
        let k = i64::from(self.interval.max(1));
        let by_day_ok = |d: Date| {
            self.by_day.is_empty()
                || self.by_day.iter().any(|wd| {
                    wd.weekday == d.weekday()
                        && match wd.nth {
                            None => true,
                            Some(n) if n > 0 => i64::from((d.day() - 1) / 7 + 1) == i64::from(n),
                            Some(n) => {
                                let from_end = (d.days_in_month() - d.day()) / 7 + 1;
                                i64::from(from_end) == -i64::from(n)
                            }
                        }
                })
        };
        let month_day_ok = |d: Date| {
            self.by_month_day.is_empty()
                || self.by_month_day.iter().any(|&m| {
                    if m > 0 {
                        i64::from(d.day()) == i64::from(m)
                    } else {
                        i64::from(d.days_in_month() - d.day() + 1) == -i64::from(m)
                    }
                })
        };
        let months = |a: Date, b: Date| {
            (i64::from(b.year()) * 12 + i64::from(b.month()))
                - (i64::from(a.year()) * 12 + i64::from(a.month()))
        };
        match self.freq {
            Frequency::Daily => days_between(dtstart, d) % k == 0,
            Frequency::Weekly => {
                let days: Vec<Weekday> = if self.by_day.is_empty() {
                    vec![dtstart.weekday()]
                } else {
                    self.by_day.iter().map(|w| w.weekday).collect()
                };
                let week0 = monday_of(dtstart);
                days.contains(&d.weekday()) && (days_between(week0, d) / 7) % k == 0
            }
            Frequency::Monthly => {
                months(dtstart, d) % k == 0
                    && if self.by_day.is_empty() && self.by_month_day.is_empty() {
                        d.day() == dtstart.day()
                    } else {
                        by_day_ok(d) && month_day_ok(d)
                    }
            }
            Frequency::Yearly => {
                (i64::from(d.year()) - i64::from(dtstart.year())) % k == 0
                    && d.month() == dtstart.month()
                    && d.day() == dtstart.day()
            }
        }
    }

    /// Every date the rule produces in `[dtstart, last]`.
    pub fn dates_through(&self, dtstart: Date, last: Date) -> Vec<Date> {
        let mut out = Vec::new();
        let mut d = dtstart;
        while d <= last {
            if self.matches(dtstart, d) {
                out.push(d);
            }
            match d.tomorrow() {
                Ok(n) => d = n,
                Err(_) => break,
            }
        }
        out
    }
}

fn days_between(a: Date, b: Date) -> i64 {
    a.until(b).map(|s| i64::from(s.get_days())).unwrap_or(0)
}

fn monday_of(d: Date) -> Date {
    d.checked_sub(i64::from(d.weekday().to_monday_zero_offset()).days())
        .unwrap_or(d)
}

/// How much to trust an inferred rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// The rule reproduces every occurrence exactly (≥ 4 occurrences).
    Exact,
    /// The rule plus a few `EXDATE`/`RDATE` corrections reproduces every occurrence.
    WithExceptions,
    /// Only a few occurrences: several rules could fit, the simplest was chosen.
    Weak,
}

/// A recurrence recovered from occurrences.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InferredRecurrence {
    pub rule: RRule,
    /// First occurrence.
    #[serde(serialize_with = "ser_zoned")]
    pub dtstart: Zoned,
    /// Length of each occurrence.
    pub duration: SignedDuration,
    /// Dates the rule produces that the site does not have.
    #[serde(serialize_with = "ser_zoned_vec")]
    pub exdates: Vec<Zoned>,
    /// Dates the site has that the rule does not produce.
    #[serde(serialize_with = "ser_zoned_vec")]
    pub rdates: Vec<Zoned>,
    pub confidence: Confidence,
    pub occurrences: usize,
}

fn ser_zoned<S: Serializer>(z: &Zoned, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(&z.strftime("%Y-%m-%dT%H:%M:%S%:z"))
}

fn ser_zoned_vec<S: Serializer>(v: &[Zoned], s: S) -> Result<S::Ok, S::Error> {
    s.collect_seq(
        v.iter()
            .map(|z| z.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string()),
    )
}

impl InferredRecurrence {
    /// iCalendar lines for a VEVENT body: `DTSTART`, `DURATION`, `RRULE`, `EXDATE`, `RDATE`.
    pub fn to_ical(&self) -> String {
        let tzid = self.dtstart.time_zone().iana_name();
        let fmt = |z: &Zoned| match tzid {
            Some(_) => z.strftime("%Y%m%dT%H%M%S").to_string(),
            None => z.timestamp().strftime("%Y%m%dT%H%M%SZ").to_string(),
        };
        let prop = |name: &str| match tzid {
            Some(t) => format!("{name};TZID={t}:"),
            None => format!("{name}:"),
        };
        let mut out = format!(
            "{}{}\r\nDURATION:{}\r\nRRULE:{}\r\n",
            prop("DTSTART"),
            fmt(&self.dtstart),
            ical_duration(self.duration),
            self.rule
        );
        for (name, dates) in [("EXDATE", &self.exdates), ("RDATE", &self.rdates)] {
            if !dates.is_empty() {
                let list: Vec<String> = dates.iter().map(fmt).collect();
                out.push_str(&format!("{}{}\r\n", prop(name), list.join(",")));
            }
        }
        out
    }
}

fn ical_duration(d: SignedDuration) -> String {
    let secs = d.as_secs().max(0);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let mut out = String::from("PT");
    if h > 0 {
        out.push_str(&format!("{h}H"));
    }
    if m > 0 {
        out.push_str(&format!("{m}M"));
    }
    if s > 0 || out == "PT" {
        out.push_str(&format!("{s}S"));
    }
    out
}

/// Tuning for [`infer`].
#[derive(Debug, Clone, Copy)]
pub struct InferOptions {
    /// Fewer occurrences than this → no inference. Default 3.
    pub min_occurrences: usize,
    /// Reject a fit when `EXDATE`s + `RDATE`s exceed this fraction of occurrences. Default ⅓.
    pub max_correction_ratio: f64,
}

impl Default for InferOptions {
    fn default() -> Self {
        Self {
            min_occurrences: 3,
            max_correction_ratio: 1.0 / 3.0,
        }
    }
}

/// One observed occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub start: Zoned,
    pub end: Option<Zoned>,
}

/// Infer a rule. Returns `None` with too few occurrences, when start times or durations vary,
/// or when no candidate explains the dates within the correction budget.
pub fn infer(occurrences: &[Observed], opts: InferOptions) -> Option<InferredRecurrence> {
    if occurrences.len() < opts.min_occurrences.max(2) {
        return None;
    }
    let mut occ: Vec<&Observed> = occurrences.iter().collect();
    occ.sort_by(|a, b| a.start.cmp(&b.start));
    let tz = occ[0].start.time_zone().clone();
    let local: Vec<Zoned> = occ
        .iter()
        .map(|o| o.start.with_time_zone(tz.clone()))
        .collect();
    let time: Time = local[0].time();
    if local.iter().any(|z| z.time() != time) {
        return None;
    }
    let durations: Vec<Option<SignedDuration>> = occ
        .iter()
        .map(|o| o.end.as_ref().map(|e| o.start.duration_until(e)))
        .collect();
    let duration = durations[0].unwrap_or(SignedDuration::ZERO);
    if durations
        .iter()
        .any(|d| d.unwrap_or(SignedDuration::ZERO) != duration)
    {
        return None;
    }
    let mut dates: Vec<Date> = local.iter().map(Zoned::date).collect();
    dates.dedup();
    let (first, last) = (dates[0], *dates.last()?);
    let have: std::collections::BTreeSet<Date> = dates.iter().copied().collect();

    struct Fit {
        score: (usize, usize),
        rule: RRule,
        missing: Vec<Date>,
        extra: Vec<Date>,
    }
    let mut best: Option<Fit> = None;
    for (rank, rule) in candidates(&dates).into_iter().enumerate() {
        let generated: std::collections::BTreeSet<Date> =
            rule.dates_through(first, last).into_iter().collect();
        let missing: Vec<Date> = generated.difference(&have).copied().collect();
        let extra: Vec<Date> = have.difference(&generated).copied().collect();
        let score = (missing.len() + 2 * extra.len(), rank);
        if best.as_ref().is_none_or(|b| score < b.score) {
            best = Some(Fit {
                score,
                rule,
                missing,
                extra,
            });
        }
    }
    let Fit {
        mut rule,
        missing,
        extra,
        ..
    } = best?;
    let corrections = missing.len() + extra.len();
    let budget = ((dates.len() as f64) * opts.max_correction_ratio)
        .floor()
        .max(1.0) as usize;
    if corrections > budget {
        return None;
    }
    let at = |d: Date| d.to_datetime(time).to_zoned(tz.clone()).ok();
    rule.until = at(last).map(|z| z.timestamp());
    let confidence = if dates.len() < 4 {
        Confidence::Weak
    } else if corrections == 0 {
        Confidence::Exact
    } else {
        Confidence::WithExceptions
    };
    Some(InferredRecurrence {
        rule,
        dtstart: at(first)?,
        duration,
        exdates: missing.into_iter().filter_map(at).collect(),
        rdates: extra.into_iter().filter_map(at).collect(),
        confidence,
        occurrences: dates.len(),
    })
}

/// Candidate rules, simplest first (earlier wins ties).
fn candidates(dates: &[Date]) -> Vec<RRule> {
    let first = dates[0];
    let mut wds: Vec<Weekday> = dates.iter().map(|d| d.weekday()).collect();
    wds.sort_by_key(|w| w.to_monday_zero_offset());
    wds.dedup();
    let rule = |freq, interval, by_day: Vec<WeekdayNum>, by_month_day: Vec<i8>| RRule {
        freq,
        interval,
        by_day,
        by_month_day,
        until: None,
    };
    let mut out = Vec::new();
    // DAILY with INTERVAL=7·n duplicates WEEKLY, so stop at 6.
    for k in 1..=6 {
        out.push(rule(Frequency::Daily, k, vec![], vec![]));
    }
    let plain: Vec<WeekdayNum> = wds
        .iter()
        .map(|&weekday| WeekdayNum { nth: None, weekday })
        .collect();
    for k in 1..=4 {
        out.push(rule(Frequency::Weekly, k, plain.clone(), vec![]));
    }
    for k in 1..=3 {
        out.push(rule(Frequency::Monthly, k, vec![], vec![first.day()]));
    }
    if let [weekday] = wds[..] {
        let nths: std::collections::BTreeSet<i8> =
            dates.iter().map(|d| (d.day() - 1) / 7 + 1).collect();
        let all_last = dates.iter().all(|d| d.days_in_month() - d.day() < 7);
        for k in 1..=3 {
            if let [n] = nths.iter().copied().collect::<Vec<_>>()[..] {
                out.push(rule(
                    Frequency::Monthly,
                    k,
                    vec![WeekdayNum {
                        nth: Some(n),
                        weekday,
                    }],
                    vec![],
                ));
            }
            if all_last {
                out.push(rule(
                    Frequency::Monthly,
                    k,
                    vec![WeekdayNum {
                        nth: Some(-1),
                        weekday,
                    }],
                    vec![],
                ));
            }
        }
    }
    out.push(rule(Frequency::Yearly, 1, vec![], vec![]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::tz::TimeZone;

    fn obs(dates: &[&str], hm: (i8, i8), mins: i64) -> Vec<Observed> {
        let tz = TimeZone::get("America/Chicago").unwrap();
        dates
            .iter()
            .map(|d| {
                let start = d
                    .parse::<Date>()
                    .unwrap()
                    .at(hm.0, hm.1, 0, 0)
                    .to_zoned(tz.clone())
                    .unwrap();
                let end = start.checked_add(mins.minutes()).unwrap();
                Observed {
                    start,
                    end: Some(end),
                }
            })
            .collect()
    }

    #[test]
    fn monthly_second_thursday_across_dst() {
        let o = obs(
            &[
                "2026-07-09",
                "2026-08-13",
                "2026-09-10",
                "2026-10-08",
                "2026-11-12",
                "2026-12-10",
                "2027-01-14",
                "2027-02-11",
                "2027-03-11",
                "2027-04-08",
            ],
            (17, 30),
            150,
        );
        let r = infer(&o, InferOptions::default()).unwrap();
        assert_eq!(r.confidence, Confidence::Exact);
        assert_eq!(
            r.rule.to_string(),
            "FREQ=MONTHLY;BYDAY=2TH;UNTIL=20270408T223000Z"
        );
        assert_eq!(r.duration, SignedDuration::from_mins(150));
        assert!(
            r.to_ical()
                .contains("DTSTART;TZID=America/Chicago:20260709T173000\r\nDURATION:PT2H30M\r\n")
        );
    }

    #[test]
    fn weekly_with_holiday_exdates() {
        let o = obs(
            &[
                "2024-11-01",
                "2024-11-08",
                "2024-11-15",
                "2024-11-22",
                "2024-12-06",
                "2024-12-13",
            ],
            (10, 0),
            240,
        );
        let r = infer(&o, InferOptions::default()).unwrap();
        assert_eq!(r.rule.freq, Frequency::Weekly);
        assert_eq!(r.confidence, Confidence::WithExceptions);
        assert_eq!(r.exdates.len(), 1);
        assert_eq!(r.exdates[0].date().to_string(), "2024-11-29");
    }

    #[test]
    fn last_friday_and_biweekly() {
        let o = obs(
            &[
                "2024-02-23",
                "2024-03-29",
                "2024-04-26",
                "2024-05-31",
                "2024-06-28",
            ],
            (11, 0),
            120,
        );
        assert_eq!(
            infer(&o, InferOptions::default())
                .unwrap()
                .rule
                .to_string()
                .split(";UNTIL")
                .next(),
            Some("FREQ=MONTHLY;BYDAY=-1FR")
        );
        let o = obs(
            &["2023-11-03", "2023-11-17", "2023-12-01", "2023-12-15"],
            (10, 0),
            120,
        );
        assert_eq!(
            infer(&o, InferOptions::default())
                .unwrap()
                .rule
                .to_string()
                .split(";UNTIL")
                .next(),
            Some("FREQ=WEEKLY;INTERVAL=2;BYDAY=FR")
        );
    }

    #[test]
    fn irregular_and_inconsistent_are_rejected() {
        let o = obs(
            &["2024-02-09", "2024-03-22", "2024-04-19", "2024-05-17"],
            (10, 0),
            120,
        );
        assert!(infer(&o, InferOptions::default()).is_none());
        let mut o = obs(&["2024-01-02", "2024-01-09", "2024-01-16"], (13, 0), 60);
        o[1].start = o[1].start.checked_add(1.hour()).unwrap();
        assert!(infer(&o, InferOptions::default()).is_none());
        assert!(infer(&o[..2], InferOptions::default()).is_none());
    }
}
