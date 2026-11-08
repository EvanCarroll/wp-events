//! A compact, API-independent view of an event.
//!
//! Text has HTML entities decoded, times are zoned instants, venues and organizers are reduced
//! to what a listing needs, and the post-bookkeeping fields are dropped. Use the full models in
//! [`crate::tribe::models`] / [`crate::tec::models`] when you need everything.

use jiff::Zoned;
use jiff::tz::TimeZone;
use serde::{Serialize, Serializer};
use url::Url;

use crate::permalink::SeriesSlug;
use crate::tec::models as tec;
use crate::tribe::models as tribe;
use crate::types::{EventId, Html, PostStatus, TermId};

/// One event occurrence.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventSummary {
    /// Unique per occurrence (for recurring events, the occurrence's own id).
    pub id: EventId,
    /// The recurring series this occurrence belongs to (both APIs, from the permalink).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<SeriesSlug>,
    /// The series parent post, when the API reports it (tec/v1 with Pro).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_parent: Option<EventId>,
    pub title: String,
    /// Start in the event's zone, serialised as `2026-11-03T16:00:00-06:00`.
    #[serde(serialize_with = "ser_zoned")]
    pub start: Option<Zoned>,
    #[serde(serialize_with = "ser_zoned")]
    pub end: Option<Zoned>,
    pub all_day: bool,
    /// IANA zone name (or a fixed-offset label) for `start`/`end`.
    pub timezone: String,
    pub url: Option<Url>,
    /// Display cost, e.g. `$100.00`, `Free`; `None` when not set.
    pub cost: Option<String>,
    pub venue: Option<VenueSummary>,
    pub organizers: Vec<String>,
    pub categories: Vec<TermSummary>,
    pub tags: Vec<TermSummary>,
    /// Full-size featured image (legacy API only; tec/v1 exposes just an attachment id).
    pub image: Option<Url>,
    pub status: PostStatus,
}

/// Where an event happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VenueSummary {
    pub name: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    /// State, province or region.
    pub region: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
}

/// A category or tag. tec/v1 only returns ids, so `name` is `None` there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TermSummary {
    pub id: TermId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

fn ser_zoned<S: Serializer>(v: &Option<Zoned>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(z) => s.collect_str(&z.strftime("%Y-%m-%dT%H:%M:%S%:z")),
        None => s.serialize_none(),
    }
}

/// Decode HTML entities (`&#038;` → `&`) and trim.
pub fn decode_text(s: &str) -> String {
    html_escape::decode_html_entities(s).trim().to_owned()
}

fn text(h: &Html) -> Option<String> {
    Some(h.to_text()).filter(|s| !s.is_empty())
}

fn plain(s: Option<&str>) -> Option<String> {
    s.map(decode_text).filter(|s| !s.is_empty())
}

fn zone_label(tz: &TimeZone) -> String {
    tz.iana_name()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{tz:?}"))
}

impl EventSummary {
    /// Summarise a legacy event. `fallback_tz` is used when the event has no usable zone
    /// (normally the site's zone).
    pub fn from_tribe(e: &tribe::Event, fallback_tz: &TimeZone) -> Self {
        let tz = e.time_zone(fallback_tz);
        let start = e
            .start_zoned(fallback_tz)
            .or_else(|| e.start_date.and_then(|d| d.to_zoned(tz.clone()).ok()));
        let end = e
            .end_zoned(fallback_tz)
            .or_else(|| e.end_date.and_then(|d| d.to_zoned(tz.clone()).ok()));
        let venue = e.venues.first().map(|v| VenueSummary {
            name: v.name.as_ref().and_then(text),
            address: plain(v.address.as_deref()),
            city: plain(v.city.as_deref()),
            region: plain(
                v.stateprovince
                    .as_deref()
                    .or(v.state.as_deref())
                    .or(v.province.as_deref()),
            ),
            postal_code: plain(v.zip.as_deref()),
            country: plain(v.country.as_deref()),
        });
        let term = |t: &tribe::Term| TermSummary {
            id: t.id,
            name: text(&t.name),
        };
        Self {
            id: e.id,
            series: e.permalink().and_then(|p| p.series()),
            series_parent: None,
            title: e.title.to_text(),
            start,
            end,
            all_day: e.all_day.unwrap_or(false),
            timezone: zone_label(&tz),
            url: e.url.clone(),
            cost: text(&e.cost),
            venue,
            organizers: e
                .organizers
                .iter()
                .filter_map(|o| o.name.as_ref().and_then(text))
                .collect(),
            categories: e.categories.iter().map(term).collect(),
            tags: e.tags.iter().map(term).collect(),
            image: e.image.as_ref().and_then(|i| i.url.clone()),
            status: e.status.clone(),
        }
    }

    /// Summarise a tec/v1 event. Returns `None` for password-protected events.
    pub fn from_tec(item: &tec::Protectable<tec::Event>, fallback_tz: &TimeZone) -> Option<Self> {
        let e = item.visible()?;
        let tz = e
            .timezone
            .to_time_zone()
            .unwrap_or_else(|| fallback_tz.clone());
        let from_utc = |d: Option<jiff::civil::DateTime>| {
            d.and_then(|d| d.to_zoned(TimeZone::UTC).ok())
                .map(|z| z.with_time_zone(tz.clone()))
        };
        let start = e.start_zoned().or_else(|| from_utc(e.start_date_utc));
        let end = e.end_zoned().or_else(|| from_utc(e.end_date_utc));
        let venue = e
            .venues
            .iter()
            .find_map(tec::Protectable::visible)
            .map(|v| VenueSummary {
                name: text(&v.post.title.rendered),
                address: plain(v.address.as_deref()),
                city: plain(v.city.as_deref()),
                region: plain(
                    v.state_province
                        .as_deref()
                        .or(v.state.as_deref())
                        .or(v.province.as_deref()),
                ),
                postal_code: plain(v.zip.as_deref()),
                country: plain(v.country.as_deref()),
            });
        let ids = |v: &[TermId]| v.iter().map(|&id| TermSummary { id, name: None }).collect();
        let timezone = start
            .as_ref()
            .map_or_else(|| zone_label(&tz), |z| zone_label(z.time_zone()));
        Some(Self {
            id: e.occurrence_id.unwrap_or(e.post.id),
            series: e.permalink().and_then(|p| p.series()),
            series_parent: e.series_parent(),
            title: e.post.title.rendered.to_text(),
            start,
            end,
            all_day: e.all_day,
            timezone,
            url: e.post.link.clone(),
            cost: text(&e.cost),
            venue,
            organizers: e.organizer_names.iter().filter_map(text).collect(),
            categories: ids(&e.categories),
            tags: ids(&e.tags),
            image: None,
            status: e.post.status.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_entities() {
        assert_eq!(
            decode_text("Wine Tasting Class &#038; Chocolate"),
            "Wine Tasting Class & Chocolate"
        );
        assert_eq!(decode_text("&#036;100.00"), "$100.00");
        assert_eq!(
            decode_text("Rock &amp; Roll&#8217;s"),
            "Rock & Roll\u{2019}s"
        );
    }

    #[test]
    fn legacy_summary_from_live_fixture() {
        let raw = include_str!("../tests/fixtures/live_demo_events.json");
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        let events: Vec<tribe::Event> = serde_json::from_value(v["events"].clone()).unwrap();
        let s = EventSummary::from_tribe(&events[0], &TimeZone::UTC);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["title"], "Hosted Dinner with Chef Monica Geller");
        assert_eq!(json["start"], "2026-11-09T18:30:00-05:00");
        assert_eq!(json["timezone"], "America/Detroit");
        assert_eq!(json["cost"], "$100.00");
        assert!(json["venue"]["name"].is_string());
        assert!(json["categories"][0]["name"].is_string());
        assert!(json.get("series_parent").is_none());
    }
}
