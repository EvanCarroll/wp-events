//! Response models for `tec/v1`.
//!
//! Entities are WordPress core post responses plus TEC fields. Password-protected posts are
//! rewritten so aggressively by the server (strings → `"Password protected"`, dates → 1970) that
//! they are surfaced as [`Protectable::PasswordProtected`] instead of half-valid data.

use std::fmt::{Debug, Display};
use std::hash::Hash;

use jiff::civil;
use jiff::tz::{Offset, TimeZone};
use jiff::{SignedDuration, Timestamp, Zoned};
use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::permalink::EventPermalink;
use crate::types::{
    AttachmentId, EventId, Html, OrganizerId, PostStatus, PostType, TermId, Ticketed, UserId,
    VenueId, WebLink, WpTimeZone,
};
use crate::wire;

/// A tec/v1 entity: event, venue or organizer.
pub trait Entity {
    /// The entity's typed post id.
    type Id: Copy
        + Eq
        + Hash
        + Debug
        + Display
        + Serialize
        + DeserializeOwned
        + Send
        + Sync
        + 'static;

    fn id(&self) -> Self::Id;

    /// Per-row identity. For events of a recurring series every occurrence shares the parent's
    /// [`Entity::id`]; this distinguishes them.
    fn row_key(&self) -> RowKey {
        RowKey {
            post: id_u64(&self.id()),
            occurrence: None,
            start: None,
        }
    }
}

fn id_u64<I: Serialize>(id: &I) -> u64 {
    serde_json::to_value(id)
        .ok()
        .and_then(|v| v.as_u64())
        .unwrap_or_default()
}

/// Unique identity of one returned row (used for de-duplication).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RowKey {
    post: u64,
    occurrence: Option<u64>,
    start: Option<civil::DateTime>,
}

/// Either a readable entity or a password-protected stub.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)] // `T` is the large one in practice.
pub enum Protectable<T: Entity> {
    Visible(T),
    /// The post needs a password; only whitelisted core fields are trustworthy.
    PasswordProtected(ProtectedPost<T::Id>),
}

impl<T: Entity> Protectable<T> {
    pub fn visible(&self) -> Option<&T> {
        match self {
            Protectable::Visible(t) => Some(t),
            Protectable::PasswordProtected(_) => None,
        }
    }

    pub fn into_visible(self) -> Option<T> {
        match self {
            Protectable::Visible(t) => Some(t),
            Protectable::PasswordProtected(_) => None,
        }
    }

    pub fn id(&self) -> T::Id {
        match self {
            Protectable::Visible(t) => t.id(),
            Protectable::PasswordProtected(p) => p.id,
        }
    }

    /// Unique per returned row, even across occurrences of one recurring series.
    pub fn row_key(&self) -> RowKey {
        match self {
            Protectable::Visible(t) => t.row_key(),
            Protectable::PasswordProtected(p) => RowKey {
                post: id_u64(&p.id),
                occurrence: None,
                // The permalink of a protected occurrence still carries its date.
                start: p
                    .link
                    .as_ref()
                    .and_then(EventPermalink::parse)
                    .and_then(|l| l.occurrence_date)
                    .map(|d| d.at(0, 0, 0, 0)),
            },
        }
    }
}

impl<'de, T> Deserialize<'de> for Protectable<T>
where
    T: Entity + DeserializeOwned,
{
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        // Mangled posts have `content.protected: null` (the bool was nulled) and, for
        // viewable types, a `post-password-required` class.
        let mangled = v
            .get("content")
            .and_then(|c| c.get("protected"))
            .is_some_and(serde_json::Value::is_null)
            || v.get("class_list")
                .and_then(|c| c.as_array())
                .is_some_and(|a| a.iter().any(|c| c == "post-password-required"));
        if mangled {
            serde_json::from_value(v)
                .map(Protectable::PasswordProtected)
                .map_err(serde::de::Error::custom)
        } else {
            serde_json::from_value(v)
                .map(Protectable::Visible)
                .map_err(serde::de::Error::custom)
        }
    }
}

/// The fields the server leaves intact on password-protected posts.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProtectedPost<I> {
    pub id: I,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub date: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub date_gmt: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub modified: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub modified_gmt: Option<civil::DateTime>,
    #[serde(default)]
    pub slug: String,
    pub status: PostStatus,
    #[serde(rename = "type")]
    pub post_type: PostType,
    #[serde(default, deserialize_with = "wire::url_opt")]
    pub link: Option<Url>,
    pub title: Rendered,
    #[serde(default, deserialize_with = "wire::id_zero_none")]
    pub author: Option<UserId>,
    /// Always `true`; present so serialised output is self-describing.
    #[serde(default = "yes")]
    pub password_protected: bool,
}

fn yes() -> bool {
    true
}

/// `{ "rendered": "..." }`, plus `raw` in `context=edit`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Rendered {
    pub rendered: Html,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

/// `{ "rendered": "...", "protected": bool }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RenderedProtected {
    pub rendered: Html,
    /// The post has a password (it was supplied, or you can edit it).
    #[serde(default)]
    pub protected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_version: Option<u32>,
}

/// WordPress core post fields shared by events, venues and organizers.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PostCore<I> {
    pub id: I,
    /// Publish date, site-local.
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub date: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub date_gmt: Option<civil::DateTime>,
    pub guid: Option<Rendered>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub modified: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub modified_gmt: Option<civil::DateTime>,
    #[serde(default)]
    pub slug: String,
    pub status: PostStatus,
    #[serde(rename = "type")]
    pub post_type: PostType,
    /// Permalink. Drafts get `?post_type=…&p=ID` links; recurring occurrences
    /// `/event/{series-slug}/{date}/`.
    #[serde(default, deserialize_with = "wire::url_opt")]
    pub link: Option<Url>,
    pub title: Rendered,
    pub content: Option<RenderedProtected>,
    pub excerpt: Option<RenderedProtected>,
    #[serde(default, deserialize_with = "wire::id_zero_none")]
    pub author: Option<UserId>,
    #[serde(default, deserialize_with = "wire::id_zero_none")]
    pub featured_media: Option<AttachmentId>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub template: Option<String>,
    /// `context=edit` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permalink_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_slug: Option<String>,
}

/// An event row. With Events Calendar Pro, one row per **occurrence**: `post.id` is the
/// series parent, [`Event::occurrence_id`] the occurrence.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Event {
    #[serde(flatten)]
    pub post: PostCore<EventId>,
    /// `post_tag` term ids.
    #[serde(default)]
    pub tags: Vec<TermId>,
    /// Event category term ids.
    #[serde(default, rename = "tribe_events_cat")]
    pub categories: Vec<TermId>,
    #[serde(default)]
    pub class_list: Vec<String>,
    /// Wall-clock start in [`Event::timezone`].
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub start_date: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub start_date_utc: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub end_date: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "wire::civil_datetime_opt")]
    pub end_date_utc: Option<civil::DateTime>,
    pub dates: Option<DateDetails>,
    #[serde(default)]
    pub timezone: WpTimeZone,
    /// Sent as a numeric string of seconds.
    #[serde(default, deserialize_with = "duration_opt")]
    pub duration: Option<SignedDuration>,
    /// Number of days spanned, or `None` for single-day events.
    #[serde(default, deserialize_with = "wire::false_or_u32")]
    pub multiday: Option<u32>,
    #[serde(default)]
    pub is_past: bool,
    #[serde(default)]
    pub is_now: bool,
    #[serde(default)]
    pub all_day: bool,
    /// Always `null` over REST in TEC ≤ 6.18.
    #[serde(default)]
    pub starts_this_week: Option<bool>,
    #[serde(default)]
    pub ends_this_week: Option<bool>,
    #[serde(default)]
    pub this_week_duration: Option<u32>,
    #[serde(default)]
    pub happens_this_week: Option<bool>,
    /// Always empty over REST in TEC ≤ 6.18.
    #[serde(default, deserialize_with = "wire::civil_dates")]
    pub displays_on: Vec<civil::Date>,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub sticky: bool,
    /// Display string, e.g. `$99.00` or `Free`; entity-encoded.
    #[serde(default)]
    pub cost: Html,
    /// Names of all linked organizers, including ones you cannot read; entity-encoded.
    #[serde(default)]
    pub organizer_names: Vec<Html>,
    #[serde(default)]
    pub organizers: Vec<Protectable<Organizer>>,
    #[serde(default)]
    pub venues: Vec<Protectable<Venue>>,
    #[serde(default)]
    pub schedule_details: Html,
    #[serde(default)]
    pub short_schedule_details: Html,
    /// Events Calendar Pro: this occurrence's own id (the legacy API's id for it).
    /// `None` without Pro. Not accepted by `GET /events/{id}`.
    #[serde(
        default,
        deserialize_with = "wire::id_zero_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub occurrence_id: Option<EventId>,
    /// Event Tickets status; `None` without the plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticketed: Option<Ticketed>,
    /// Extra taxonomies and add-on fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn duration_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SignedDuration>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum In {
        N(i64),
        S(String),
    }
    Ok(match Option::<In>::deserialize(d)? {
        Some(In::N(n)) => Some(SignedDuration::from_secs(n)),
        Some(In::S(s)) => s.trim().parse().ok().map(SignedDuration::from_secs),
        None => None,
    })
}

impl Entity for Event {
    type Id = EventId;
    fn id(&self) -> EventId {
        self.post.id
    }
    fn row_key(&self) -> RowKey {
        RowKey {
            post: self.post.id.get(),
            occurrence: self.occurrence_id.map(EventId::get),
            start: self.start_date_utc.or(self.start_date),
        }
    }
}

impl Event {
    /// The post id. **For a recurring series this is the parent's id, shared by every
    /// occurrence**; see [`Event::occurrence_id`] and [`Event::series_parent`].
    pub fn id(&self) -> EventId {
        self.post.id
    }

    /// `true` for an occurrence of a recurring series.
    pub fn is_recurring(&self) -> bool {
        self.class_list.iter().any(|c| c == "tribe-recurring-event")
    }

    /// The recurring series parent post this occurrence belongs to.
    pub fn series_parent(&self) -> Option<EventId> {
        self.is_recurring().then_some(self.post.id)
    }

    /// The parsed permalink (carries the series slug and occurrence date).
    pub fn permalink(&self) -> Option<EventPermalink> {
        EventPermalink::parse(self.post.link.as_ref()?)
    }

    pub fn start_instant(&self) -> Option<Timestamp> {
        self.start_date_utc?
            .to_zoned(TimeZone::UTC)
            .ok()
            .map(|z| z.timestamp())
    }

    pub fn end_instant(&self) -> Option<Timestamp> {
        self.end_date_utc?
            .to_zoned(TimeZone::UTC)
            .ok()
            .map(|z| z.timestamp())
    }

    /// Start in the event's own zone (from `dates.start`).
    pub fn start_zoned(&self) -> Option<Zoned> {
        self.dates.as_ref()?.start.zoned()
    }

    pub fn end_zoned(&self) -> Option<Zoned> {
        self.dates.as_ref()?.end.zoned()
    }
}

/// The eight PHP `DateTimeImmutable` values in `Event.dates`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DateDetails {
    /// Event zone.
    pub start: PhpDate,
    pub start_utc: PhpDate,
    /// Site zone.
    pub start_site: PhpDate,
    /// The zone the site displays (event or site zone, per settings).
    pub start_display: PhpDate,
    pub end: PhpDate,
    pub end_utc: PhpDate,
    pub end_site: PhpDate,
    pub end_display: PhpDate,
}

/// How a PHP `DateTimeImmutable` names its zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum PhpTimezoneType {
    /// `+02:00`
    Offset,
    /// `EST` (ambiguous)
    Abbreviation,
    /// `Europe/Athens`
    Identifier,
    /// `0`: the value was blanked (password protection) or unknown.
    Unknown(u8),
}

impl<'de> Deserialize<'de> for PhpTimezoneType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match u8::deserialize(d)? {
            1 => PhpTimezoneType::Offset,
            2 => PhpTimezoneType::Abbreviation,
            3 => PhpTimezoneType::Identifier,
            n => PhpTimezoneType::Unknown(n),
        })
    }
}

/// PHP's JSON form of `DateTimeImmutable`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PhpDate {
    #[serde(deserialize_with = "wire::civil_datetime")]
    pub date: civil::DateTime,
    pub timezone_type: PhpTimezoneType,
    pub timezone: String,
}

impl PhpDate {
    /// Resolve to a zoned date-time. `None` for abbreviation zones (ambiguous) or unknown ids.
    pub fn zoned(&self) -> Option<Zoned> {
        let tz = match self.timezone_type {
            PhpTimezoneType::Offset => TimeZone::fixed(parse_offset(&self.timezone)?),
            PhpTimezoneType::Identifier => TimeZone::get(&self.timezone).ok()?,
            _ => return None,
        };
        self.date.to_zoned(tz).ok()
    }
}

fn parse_offset(s: &str) -> Option<Offset> {
    let (sign, rest) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    let (h, m) = rest.split_once(':').unwrap_or((rest, "0"));
    let secs = sign * (h.parse::<i32>().ok()? * 3600 + m.parse::<i32>().ok()? * 60);
    Offset::from_seconds(secs).ok()
}

/// A venue.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Venue {
    #[serde(flatten)]
    pub post: PostCore<VenueId>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub address: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub country: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub city: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub state_province: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub state: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub province: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub zip: Option<String>,
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub phone: Option<String>,
    /// Google Maps link (the server's `&#038;` escaping is undone).
    #[serde(default, deserialize_with = "directions")]
    pub directions_link: Option<Url>,
    #[serde(default, deserialize_with = "wire::weblink_opt")]
    pub website: Option<WebLink>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn directions<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Url>, D::Error> {
    match wire::empty_none(d)? {
        None => Ok(None),
        Some(s) => {
            let s = html_escape::decode_html_entities(&s);
            Url::parse(s.trim())
                .map(Some)
                .map_err(|e| serde::de::Error::custom(format!("invalid directions_link: {e}")))
        }
    }
}

impl Entity for Venue {
    type Id = VenueId;
    fn id(&self) -> VenueId {
        self.post.id
    }
}

/// An organizer.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Organizer {
    #[serde(flatten)]
    pub post: PostCore<OrganizerId>,
    /// Free text.
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub phone: Option<String>,
    #[serde(default, deserialize_with = "wire::weblink_opt")]
    pub website: Option<WebLink>,
    /// May hold several comma-separated addresses.
    #[serde(default, deserialize_with = "wire::empty_none")]
    pub email: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Entity for Organizer {
    type Id = OrganizerId;
    fn id(&self) -> OrganizerId {
        self.post.id
    }
}
