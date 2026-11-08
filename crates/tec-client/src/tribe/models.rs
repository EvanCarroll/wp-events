//! Response models for `tribe/events/v1`.
//!
//! Field names follow the wire format except where noted (`venue` → `venues`,
//! `organizer` → `organizers`, venue/organizer name → `name`). Values are normalised:
//! ids are integers even when sent as strings, `false`/`[]`/zero-dates become `None`/empty.
//!
//! Booleans are `Option<bool>` because password-protected events in archives come back with
//! those fields set to `null`.

use std::collections::BTreeMap;

use jiff::civil;
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};
use serde::{Deserialize, Serialize};

use url::Url;

use crate::permalink::EventPermalink;
use crate::types::{
    AttachmentId, EventId, GlobalId, Html, OrganizerId, PostStatus, Taxonomy, TermId, Ticketed,
    UserId, VenueId, WebLink, WpTimeZone,
};
use crate::wire;

/// An event.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Event {
    pub id: EventId,
    /// Host + path + `?id=` identifier, stable across Event Aggregator imports.
    #[serde(default, deserialize_with = "wire::false_or")]
    pub global_id: Option<GlobalId>,
    #[serde(default)]
    pub global_id_lineage: Vec<GlobalId>,
    #[serde(default, deserialize_with = "wire::id_zero_none")]
    pub author: Option<UserId>,
    pub status: PostStatus,
    /// Post publish date, site-local.
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub date: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub date_utc: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub modified: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub modified_utc: Option<civil::DateTime>,
    /// Public permalink. For recurring occurrences: `/event/{series-slug}/{date}/`
    /// (see [`Event::permalink`]).
    #[serde(default, deserialize_with = "wire::url_opt")]
    pub url: Option<Url>,
    pub rest_url: Url,
    /// Entity-encoded (`&#8217;`, `&amp;`); use `.to_text()` for display.
    pub title: Html,
    #[serde(default)]
    pub description: Html,
    #[serde(default)]
    pub excerpt: Html,
    #[serde(default)]
    pub slug: String,
    #[serde(default, deserialize_with = "wire::false_or")]
    pub image: Option<Image>,
    #[serde(default)]
    pub all_day: Option<bool>,
    /// Start, wall-clock time in the event's [`Event::timezone`].
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub start_date: Option<civil::DateTime>,
    pub start_date_details: Option<DateDetails>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub end_date: Option<civil::DateTime>,
    pub end_date_details: Option<DateDetails>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub utc_start_date: Option<civil::DateTime>,
    pub utc_start_date_details: Option<DateDetails>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub utc_end_date: Option<civil::DateTime>,
    pub utc_end_date_details: Option<DateDetails>,
    /// IANA zone (`America/New_York`), a `UTC+5.5`-style offset, or empty.
    #[serde(default)]
    pub timezone: WpTimeZone,
    #[serde(default)]
    pub timezone_abbr: String,
    /// Display string, e.g. `$10 – $25` or `Free`; entity-encoded.
    #[serde(default)]
    pub cost: Html,
    pub cost_details: Option<CostDetails>,
    /// Event website. **The server substitutes the permalink when none was entered.**
    #[serde(default, deserialize_with = "wire::weblink_opt")]
    pub website: Option<WebLink>,
    #[serde(default)]
    pub show_map: Option<bool>,
    #[serde(default)]
    pub show_map_link: Option<bool>,
    #[serde(default)]
    pub hide_from_listings: Option<bool>,
    #[serde(default)]
    pub sticky: Option<bool>,
    #[serde(default)]
    pub featured: Option<bool>,
    #[serde(default, deserialize_with = "wire::list_lenient")]
    pub categories: Vec<Term>,
    #[serde(default, deserialize_with = "wire::list_lenient")]
    pub tags: Vec<Term>,
    /// Wire key `venue`: `[]`, one object, or a list.
    #[serde(
        default,
        rename(deserialize = "venue"),
        deserialize_with = "wire::one_or_many"
    )]
    pub venues: Vec<Venue>,
    /// Wire key `organizer`.
    #[serde(
        default,
        rename(deserialize = "organizer"),
        deserialize_with = "wire::one_or_many"
    )]
    pub organizers: Vec<Organizer>,
    /// "Additional fields" keyed by meta key (e.g. `_ecp_custom_2`).
    #[serde(default, deserialize_with = "wire::empty_array_or_map")]
    pub custom_fields: BTreeMap<String, CustomField>,
    /// schema.org JSON-LD; only on single-event GETs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_ld: Option<serde_json::Value>,

    // ---- Fields added by companion plugins (absent without them) ----
    /// Event Tickets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticketed: Option<Ticketed>,
    /// Event Tickets Plus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attendance: Option<Attendance>,
    /// Virtual events (Events Calendar Pro).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_virtual: Option<bool>,
    #[serde(
        default,
        deserialize_with = "wire::weblink_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub virtual_url: Option<WebLink>,
    #[serde(
        default,
        deserialize_with = "wire::empty_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub virtual_video_source: Option<String>,

    /// Any other keys (from add-ons or newer plugin versions).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Event {
    /// The event's zone, falling back to `site_tz` when unset or unknown.
    pub fn time_zone(&self, site_tz: &TimeZone) -> TimeZone {
        self.timezone
            .to_time_zone()
            .unwrap_or_else(|| site_tz.clone())
    }

    /// The parsed permalink. `permalink()?.series()` is the recurring series this
    /// occurrence belongs to (`None` for one-off events).
    pub fn permalink(&self) -> Option<EventPermalink> {
        EventPermalink::parse(self.url.as_ref()?)
    }

    /// Start as an instant (from the stored UTC value).
    pub fn start_instant(&self) -> Option<Timestamp> {
        utc_instant(self.utc_start_date)
    }

    /// End as an instant (from the stored UTC value).
    pub fn end_instant(&self) -> Option<Timestamp> {
        utc_instant(self.utc_end_date)
    }

    /// Start in the event's own zone.
    pub fn start_zoned(&self, site_tz: &TimeZone) -> Option<Zoned> {
        Some(self.start_instant()?.to_zoned(self.time_zone(site_tz)))
    }

    /// End in the event's own zone.
    pub fn end_zoned(&self, site_tz: &TimeZone) -> Option<Zoned> {
        Some(self.end_instant()?.to_zoned(self.time_zone(site_tz)))
    }

    /// `true` when the archive blanked this event because it is password protected.
    pub fn is_password_blanked(&self) -> bool {
        self.all_day.is_none() && self.show_map.is_none() && self.featured.is_none()
    }
}

fn utc_instant(dt: Option<civil::DateTime>) -> Option<Timestamp> {
    dt?.to_zoned(TimeZone::UTC).ok().map(|z| z.timestamp())
}

/// Calendar components of a date (sent as zero-padded strings on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct DateDetails {
    #[serde(deserialize_with = "wire::padded")]
    pub year: u16,
    #[serde(deserialize_with = "wire::padded")]
    pub month: u8,
    #[serde(deserialize_with = "wire::padded")]
    pub day: u8,
    #[serde(deserialize_with = "wire::padded")]
    pub hour: u8,
    #[serde(deserialize_with = "wire::padded")]
    pub minutes: u8,
    #[serde(deserialize_with = "wire::padded")]
    pub seconds: u8,
}

crate::string_enum! {
    /// Where the currency symbol goes.
    pub enum CurrencyPosition {
        /// No position stored.
        Unset = "",
        Prefix = "prefix",
        /// The admin UI writes `suffix`, REST-created events `postfix`.
        Suffix = "suffix" | "postfix",
    }
}

/// Structured cost information.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct CostDetails {
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub currency_symbol: Option<String>,
    /// ISO 4217 code.
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub currency_code: Option<String>,
    pub currency_position: CurrencyPosition,
    /// Sorted cost values.
    #[serde(default, deserialize_with = "cost_values")]
    pub values: Vec<CostValue>,
}

/// One entry of [`CostDetails::values`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CostValue {
    /// A decimal amount as text (`"10"`, `"25.5"`); text avoids float rounding of money.
    Amount(String),
    /// A slug for a non-numeric cost, e.g. `"free"`, `"donation"`.
    Label(String),
}

fn cost_values<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<CostValue>, D::Error> {
    Ok(wire::strings_or_numbers(d)?
        .into_iter()
        .map(|v| {
            let is_amount = !v.is_empty()
                && v.chars().all(|c| c.is_ascii_digit() || c == '.')
                && v.chars().filter(|&c| c == '.').count() <= 1;
            if is_amount {
                CostValue::Amount(v)
            } else {
                CostValue::Label(v)
            }
        })
        .collect())
}

/// A featured image.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Image {
    #[serde(default, deserialize_with = "wire::url_opt")]
    pub url: Option<Url>,
    pub id: AttachmentId,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub extension: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub filesize: Option<u64>,
    #[serde(default)]
    pub original_image: Option<String>,
    /// Keyed by size name (`thumbnail`, `medium`, `tribe_event_large`, ...).
    #[serde(default, deserialize_with = "wire::empty_array_or_map")]
    pub sizes: BTreeMap<String, ImageSize>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageSize {
    #[serde(deserialize_with = "wire::padded")]
    pub width: u32,
    #[serde(deserialize_with = "wire::padded")]
    pub height: u32,
    #[serde(rename = "mime-type")]
    pub mime_type: String,
    #[serde(default)]
    pub filesize: Option<u64>,
    #[serde(default, deserialize_with = "wire::url_opt")]
    pub url: Option<Url>,
}

/// A venue. The server drops every empty field, so nearly everything is optional.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Venue {
    pub id: VenueId,
    #[serde(default, deserialize_with = "wire::id_zero_none")]
    pub author: Option<UserId>,
    #[serde(default)]
    pub status: Option<PostStatus>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub date: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub date_utc: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub modified: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub modified_utc: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::url_opt")]
    pub url: Option<Url>,
    /// Venue name (wire key `venue`); entity-encoded.
    #[serde(
        default,
        rename(deserialize = "venue"),
        deserialize_with = "wire::empty_none_as"
    )]
    pub name: Option<Html>,
    #[serde(default, deserialize_with = "wire::empty_none_as")]
    pub description: Option<Html>,
    #[serde(default, deserialize_with = "wire::empty_none_as")]
    pub excerpt: Option<Html>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub slug: Option<String>,
    #[serde(default, deserialize_with = "wire::false_or")]
    pub image: Option<Image>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub address: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub city: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub country: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub province: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub state: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub stateprovince: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub zip: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub phone: Option<String>,
    #[serde(default, deserialize_with = "wire::weblink_opt")]
    pub website: Option<WebLink>,
    /// Only with Events Calendar Pro geocoding.
    #[serde(default, deserialize_with = "nonzero_f64")]
    pub geo_lat: Option<f64>,
    #[serde(default, deserialize_with = "nonzero_f64")]
    pub geo_lng: Option<f64>,
    #[serde(default)]
    pub show_map: Option<bool>,
    #[serde(default)]
    pub show_map_link: Option<bool>,
    #[serde(default, deserialize_with = "wire::false_or")]
    pub global_id: Option<GlobalId>,
    #[serde(default)]
    pub global_id_lineage: Vec<GlobalId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_ld: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// An organizer. The server drops every empty field.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Organizer {
    pub id: OrganizerId,
    #[serde(default, deserialize_with = "wire::id_zero_none")]
    pub author: Option<UserId>,
    #[serde(default)]
    pub status: Option<PostStatus>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub date: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub date_utc: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub modified: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub modified_utc: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::url_opt")]
    pub url: Option<Url>,
    /// Organizer name (wire key `organizer`); entity-encoded.
    #[serde(
        default,
        rename(deserialize = "organizer"),
        deserialize_with = "wire::empty_none_as"
    )]
    pub name: Option<Html>,
    #[serde(default, deserialize_with = "wire::empty_none_as")]
    pub description: Option<Html>,
    #[serde(default, deserialize_with = "wire::empty_none_as")]
    pub excerpt: Option<Html>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub slug: Option<String>,
    #[serde(default, deserialize_with = "wire::false_or")]
    pub image: Option<Image>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub phone: Option<String>,
    #[serde(default, deserialize_with = "wire::weblink_opt")]
    pub website: Option<WebLink>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub email: Option<String>,
    #[serde(default, deserialize_with = "wire::false_or")]
    pub global_id: Option<GlobalId>,
    #[serde(default)]
    pub global_id_lineage: Vec<GlobalId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_ld: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A category or tag. Embedded terms and `/categories` terms have slightly different
/// shapes; fields only present in one are optional.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Term {
    pub id: TermId,
    /// Entity-encoded.
    pub name: Html,
    pub slug: String,
    pub taxonomy: Taxonomy,
    #[serde(default)]
    pub description: String,
    /// Parent category (categories only; `None` at top level).
    #[serde(default, deserialize_with = "wire::id_zero_none")]
    pub parent: Option<TermId>,
    #[serde(default)]
    pub count: u64,
    /// Term archive page (only from `/categories` and `/tags`).
    #[serde(default, deserialize_with = "wire::url_opt")]
    pub url: Option<Url>,
    #[serde(default)]
    pub term_group: Option<i64>,
    #[serde(default)]
    pub term_taxonomy_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    pub urls: Option<TermUrls>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TermUrls {
    #[serde(rename = "self")]
    pub self_: Url,
    pub collection: Url,
    #[serde(default, deserialize_with = "wire::url_opt")]
    pub up: Option<Url>,
}

/// An "additional field" value.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CustomField {
    pub label: String,
    /// Checkbox values are `|`-joined.
    #[serde(deserialize_with = "wire::string_or_number")]
    pub value: String,
}

/// Event Tickets Plus attendance counts.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct Attendance {
    pub total_attendees: Option<u64>,
    pub checked_in: Option<u64>,
    pub not_checked_in: Option<u64>,
}

fn nonzero_f64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum In {
        F(f64),
        S(String),
    }
    Ok(match Option::<In>::deserialize(d)? {
        Some(In::F(f)) if f != 0.0 => Some(f),
        Some(In::S(s)) => s.trim().parse::<f64>().ok().filter(|f| *f != 0.0),
        _ => None,
    })
}

/// Archive envelope shared by all legacy list endpoints.
#[derive(Debug, Deserialize)]
pub(crate) struct ArchiveWire<T> {
    #[serde(
        alias = "events",
        alias = "venues",
        alias = "organizers",
        alias = "categories",
        alias = "tags"
    )]
    pub(crate) items: Vec<T>,
    #[serde(default, deserialize_with = "wire::u64_lenient")]
    pub(crate) total: u64,
    #[serde(default, deserialize_with = "wire::u64_lenient")]
    pub(crate) total_pages: u64,
}
