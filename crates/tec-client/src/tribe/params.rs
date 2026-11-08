//! Query and request-body types for `tribe/events/v1`.

use serde::{Serialize, Serializer};
use url::Url;

use crate::query::Query;
use crate::types::{
    AttachmentId, DateArg, EventId, Order, OrganizerId, PostStatus, TermId, TermRef, UserId,
    VenueId,
};

fn csv<T: std::fmt::Display>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Filters for `GET /events`.
///
/// **Default window:** if you set none of the date fields, the server only returns events that
/// start after yesterday and end within 24 months. Use [`crate::DateRange`] via
/// `events().crawl(...)` to get calendar-aligned windows without thinking about this.
///
/// **Date semantics (non-strict mode, the server default):** `start_date`/`end_date` snap to the
/// beginning/end of their day, and every `starts_*`/`ends_*` value snaps to 23:59:59 of its date,
/// discarding the time. Set `strict_dates: Some(true)` and pass zoned values for exact instants.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventQuery {
    pub page: Option<u32>,
    /// Silently capped at 50 by default.
    pub per_page: Option<u32>,
    /// Event start on or after.
    pub start_date: Option<DateArg>,
    /// Event end on or before.
    pub end_date: Option<DateArg>,
    pub starts_before: Option<DateArg>,
    pub starts_after: Option<DateArg>,
    pub ends_before: Option<DateArg>,
    pub ends_after: Option<DateArg>,
    pub strict_dates: Option<bool>,
    pub search: Option<String>,
    /// Any of these categories (ids or slugs).
    pub categories: Vec<TermRef>,
    /// Any of these tags (ids or slugs).
    pub tags: Vec<TermRef>,
    pub venues: Vec<VenueId>,
    pub organizers: Vec<OrganizerId>,
    /// `Some(true)` only featured, `Some(false)` only non-featured.
    pub featured: Option<bool>,
    /// Anonymous callers may only ask for `publish`.
    pub status: Vec<PostStatus>,
    /// Events Calendar Pro only.
    pub near: Option<GeoPoint>,
    /// Only these events. Note: the default date window still applies unless you set a
    /// `starts_*`/`ends_*` bound.
    pub include: Vec<EventId>,
    /// Children of a recurring series (Events Calendar Pro).
    pub post_parent: Option<EventId>,
    /// Requires Event Tickets.
    pub ticketed: Option<bool>,
    /// Unlocks password-protected events (one password for all).
    pub password: Option<String>,
    /// `context=edit`: editors see password-protected content.
    pub context_edit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    pub lat: f64,
    pub lng: f64,
}

impl EventQuery {
    pub(crate) fn to_query(&self) -> Query {
        let mut q = Query::new();
        q.opt("page", self.page)
            .opt("per_page", self.per_page)
            .opt("start_date", self.start_date.as_ref())
            .opt("end_date", self.end_date.as_ref())
            .opt("starts_before", self.starts_before.as_ref())
            .opt("starts_after", self.starts_after.as_ref())
            .opt("ends_before", self.ends_before.as_ref())
            .opt("ends_after", self.ends_after.as_ref())
            .opt(
                "strict_dates",
                self.strict_dates.map(|b| if b { "true" } else { "false" }),
            )
            .opt("search", self.search.as_deref())
            .push("categories", csv(&self.categories))
            .push("tags", csv(&self.tags))
            .push("venue", csv(&self.venues))
            .push("organizer", csv(&self.organizers))
            .opt_bool("featured", self.featured)
            .push("status", csv(&self.status))
            .push("include", csv(&self.include))
            .opt("post_parent", self.post_parent)
            .opt_bool("ticketed", self.ticketed)
            .opt("password", self.password.as_deref());
        if let Some(p) = self.near {
            q.push("geoloc", "1")
                .push("geoloc_lat", p.lat)
                .push("geoloc_lng", p.lng);
        }
        if self.context_edit {
            q.push("context", "edit");
        }
        q
    }
}

/// Filters for `GET /venues` and `GET /organizers`. Results are ordered newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkedQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    /// Matches title, description and custom fields.
    pub search: Option<String>,
    /// Only those linked to this event.
    pub event: Option<EventId>,
    /// `Some(true)`: only with events; `Some(false)`: only **without** events.
    pub has_events: Option<bool>,
    pub only_with_upcoming: Option<bool>,
    pub status: Vec<PostStatus>,
    pub password: Option<String>,
    pub context_edit: bool,
}

impl LinkedQuery {
    pub(crate) fn to_query(&self) -> Query {
        let mut q = Query::new();
        q.opt("page", self.page)
            .opt("per_page", self.per_page)
            .opt("search", self.search.as_deref())
            .opt("event", self.event)
            .opt_bool("has_events", self.has_events)
            .opt_bool("only_with_upcoming", self.only_with_upcoming)
            .push("status", csv(&self.status))
            .opt("password", self.password.as_deref());
        if self.context_edit {
            q.push("context", "edit");
        }
        q
    }
}

/// Filters for `GET /categories` and `GET /tags`.
///
/// Empty terms are **hidden by default** (`hide_empty` defaults to true server-side).
/// Pages past the end return an empty list rather than a 404.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TermQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub search: Option<String>,
    pub include: Vec<TermRef>,
    pub exclude: Vec<TermRef>,
    pub order: Option<Order>,
    pub orderby: Option<TermOrderBy>,
    pub hide_empty: Option<bool>,
    /// Categories only; rejected client-side for tags.
    pub parent: Option<TermId>,
    /// Only terms attached to this event.
    pub event: Option<EventId>,
    pub slug: Option<String>,
}

crate::string_enum! {
    /// `orderby` values understood by WordPress `get_terms`.
    pub enum TermOrderBy {
        Name = "name",
        Slug = "slug",
        TermGroup = "term_group",
        TermId = "term_id",
        Id = "id",
        Description = "description",
        Parent = "parent",
        Count = "count",
        Include = "include",
        None = "none",
    }
}

impl TermQuery {
    pub(crate) fn to_query(&self) -> Query {
        let mut q = Query::new();
        q.opt("page", self.page)
            .opt("per_page", self.per_page)
            .opt("search", self.search.as_deref())
            .push("include", csv(&self.include))
            .push("exclude", csv(&self.exclude))
            .opt("order", self.order.map(Order::as_lower))
            .opt("orderby", self.orderby.as_ref())
            .opt_bool("hide_empty", self.hide_empty)
            .opt("parent", self.parent)
            .opt("event", self.event)
            .opt("slug", self.slug.as_deref());
        q
    }
}

// ---------------------------------------------------------------------------------------------
// Write bodies
// ---------------------------------------------------------------------------------------------

/// A featured image reference for writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageRef {
    /// An existing image attachment.
    Attachment(AttachmentId),
    /// A URL the server will download (needs `upload_files`; the server HEADs it first).
    Url(Url),
}

impl Serialize for ImageRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ImageRef::Attachment(id) => s.collect_str(id),
            ImageRef::Url(u) => s.serialize_str(u.as_str()),
        }
    }
}

/// A venue for an event write: an existing one or a new one created inline.
#[derive(Debug, Clone, PartialEq)]
pub enum VenueRef {
    Existing(VenueId),
    New(Box<NewVenue>),
}

impl Serialize for VenueRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            VenueRef::Existing(id) => IdObject { id: id.0 }.serialize(s),
            VenueRef::New(v) => v.serialize(s),
        }
    }
}

/// An organizer for an event write: an existing one or a new one created inline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrganizerRef {
    Existing(OrganizerId),
    New(Box<NewOrganizer>),
}

impl Serialize for OrganizerRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            OrganizerRef::Existing(id) => IdObject { id: id.0 }.serialize(s),
            OrganizerRef::New(o) => o.serialize(s),
        }
    }
}

#[derive(Serialize)]
struct IdObject {
    id: u64,
}

/// Optional event fields shared by create and update.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EventFields {
    /// Ignored (replaced by you) without `edit_others_posts`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<UserId>,
    /// Post publish date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateArg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_utc: Option<DateArg>,
    /// HTML allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    /// Without `publish_posts`, `publish`/`future` become `pending`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<PostStatus>,
    /// Zone for the event and for interpreting `start_date`/`end_date`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_day: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageRef>,
    /// Free text, e.g. `"$10"`; currency symbol/position are parsed from it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<Url>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_map: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_map_link: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_from_listings: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sticky: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub featured: Option<bool>,
    /// Ids or names; unknown names may create terms. `Some(vec![])` clears.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<TermRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<TermRef>>,
    /// The API cannot unlink a venue; `None` leaves it unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venue: Option<VenueRef>,
    /// The API cannot unlink organizers; `None` leaves them unchanged.
    #[serde(rename = "organizer", skip_serializing_if = "Option::is_none")]
    pub organizers: Option<Vec<OrganizerRef>>,
}

/// Body for `POST /events`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewEvent {
    pub title: String,
    pub start_date: DateArg,
    pub end_date: DateArg,
    #[serde(flatten)]
    pub fields: EventFields,
}

impl NewEvent {
    pub fn new(
        title: impl Into<String>,
        start: impl Into<DateArg>,
        end: impl Into<DateArg>,
    ) -> Self {
        Self {
            title: title.into(),
            start_date: start.into(),
            end_date: end.into(),
            fields: EventFields::default(),
        }
    }
}

/// Body for `POST|PUT|PATCH /events/{id}`.
///
/// **This is not a partial update.** When omitted, the server resets `all_day`, `show_map`,
/// `show_map_link`, `sticky`, `featured` and `hide_from_listings` to false, **removes the
/// featured image**, and sets `status` to `publish` (publishing drafts). Send the full desired
/// state for those fields. Title, content, dates, cost, website, timezone and terms are kept
/// when omitted.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EventUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_date: Option<DateArg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<DateArg>,
    #[serde(flatten)]
    pub fields: EventFields,
}

fn ser_bool_01<S: Serializer>(v: &Option<bool>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(true) => s.serialize_str("1"),
        Some(false) => s.serialize_str("0"),
        None => s.serialize_none(),
    }
}

/// Optional venue fields shared by create and update.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct VenueFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<UserId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateArg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_utc: Option<DateArg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<PostStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub province: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stateprovince: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<Url>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageRef>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "ser_bool_01"
    )]
    pub show_map: Option<bool>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "ser_bool_01"
    )]
    pub show_map_link: Option<bool>,
}

/// Body for `POST /venues` (also usable inline in an event write).
///
/// With duplicate avoidance (the server default) an existing matching venue may be returned.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewVenue {
    /// The venue name.
    #[serde(rename = "venue")]
    pub name: String,
    #[serde(flatten)]
    pub fields: VenueFields,
}

impl NewVenue {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: VenueFields::default(),
        }
    }
}

/// Body for venue updates. Omitting `image` **removes** the featured image and omitting
/// `status` publishes the venue.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct VenueUpdate {
    #[serde(rename = "venue", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub fields: VenueFields,
}

/// Optional organizer fields shared by create and update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OrganizerFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<UserId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateArg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_utc: Option<DateArg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<PostStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<Url>,
    /// Not validated server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageRef>,
}

/// Body for `POST /organizers` (also usable inline in an event write).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewOrganizer {
    #[serde(rename = "organizer")]
    pub name: String,
    #[serde(flatten)]
    pub fields: OrganizerFields,
}

impl NewOrganizer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: OrganizerFields::default(),
        }
    }
}

/// Body for organizer updates. Same caveats as [`VenueUpdate`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OrganizerUpdate {
    #[serde(rename = "organizer", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub fields: OrganizerFields,
}

/// Body for creating a category or tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewTerm {
    pub name: String,
    /// Must be non-empty if set (an empty string is a 400).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// Categories only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<TermId>,
}

impl NewTerm {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            slug: None,
            parent: None,
        }
    }
}

/// Body for updating a category or tag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TermUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<TermId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_query_encoding() {
        let q = EventQuery {
            per_page: Some(50),
            categories: vec![TermRef::Id(TermId(3)), "music".into()],
            featured: Some(false),
            search: Some(String::new()),
            ..Default::default()
        }
        .to_query();
        assert_eq!(
            q.pairs(),
            &[
                ("per_page".into(), "50".into()),
                ("categories".into(), "3,music".into()),
                ("featured".into(), "0".into()),
            ]
        );
    }

    #[test]
    fn new_event_body() {
        let mut e = NewEvent::new(
            "Gala",
            jiff::civil::date(2026, 12, 1).at(19, 0, 0, 0),
            jiff::civil::date(2026, 12, 1).at(22, 0, 0, 0),
        );
        e.fields.venue = Some(VenueRef::Existing(VenueId(7)));
        e.fields.organizers = Some(vec![OrganizerRef::New(Box::new(NewOrganizer::new("Ann")))]);
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "title": "Gala",
                "start_date": "2026-12-01 19:00:00",
                "end_date": "2026-12-01 22:00:00",
                "venue": {"id": 7},
                "organizer": [{"organizer": "Ann"}],
            })
        );
    }
}
