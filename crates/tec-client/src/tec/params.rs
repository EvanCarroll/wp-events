//! Query and request-body types for `tec/v1`.

use jiff::civil;
use serde::{Serialize, Serializer};
use url::Url;

use crate::query::Query;
use crate::types::{DateArg, EventId, Order, OrganizerId, TermId, UserId, VenueId};

crate::string_enum! {
    /// Post statuses accepted by tec/v1 filters and writes.
    pub enum Status {
        Publish = "publish",
        Pending = "pending",
        Draft = "draft",
        Future = "future",
        Private = "private",
    }
}

crate::string_enum! {
    /// `orderby` for the events collection.
    pub enum EventOrderBy {
        /// Post publish date.
        Date = "date",
        /// Event start (the default).
        EventDate = "event_date",
        Title = "title",
        MenuOrder = "menu_order",
        Modified = "modified",
    }
}

/// Filters for `GET /events`. Unlike the legacy API there is **no default date window**.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventQuery {
    pub page: Option<u32>,
    /// Defaults to the site's posts-per-page (usually 10). No server maximum is enforced.
    pub per_page: Option<u32>,
    /// Event start on or after (inclusive).
    pub start_date: Option<DateArg>,
    /// Event end on or before (inclusive).
    pub end_date: Option<DateArg>,
    pub starts_before: Option<DateArg>,
    pub starts_after: Option<DateArg>,
    pub ends_before: Option<DateArg>,
    pub ends_after: Option<DateArg>,
    pub search: Option<String>,
    /// **Accepted but ignored by TEC ≤ 6.18 server-side.**
    pub categories: Vec<TermId>,
    /// **Accepted but ignored by TEC ≤ 6.18 server-side.**
    pub tags: Vec<TermId>,
    pub venues: Vec<VenueId>,
    pub organizers: Vec<OrganizerId>,
    /// `Some(true)` only featured, `Some(false)` only non-featured.
    pub featured: Option<bool>,
    /// Silently forced to `publish` for users who cannot edit events.
    pub status: Vec<Status>,
    pub post_parent: Option<EventId>,
    /// Only effective with Event Tickets installed.
    pub ticketed: Option<bool>,
    pub orderby: Option<EventOrderBy>,
    pub order: Option<Order>,
    pub context_edit: bool,
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
            .opt("search", self.search.as_deref())
            .list("categories", &self.categories)
            .list("tags", &self.tags)
            .list("venue", &self.venues)
            .list("organizer", &self.organizers)
            .opt_bool("featured", self.featured)
            .list("status", &self.status)
            .opt("post_parent", self.post_parent)
            .opt_bool("ticketed", self.ticketed)
            .opt("orderby", self.orderby.as_ref())
            .opt("order", self.order.map(Order::as_upper));
        if self.context_edit {
            q.push("context", "edit");
        }
        q
    }
}

/// Filters for `GET /venues` and `GET /organizers` (ordered newest first; no ordering params).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkedQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub search: Option<String>,
    /// Only those linked to this event.
    pub event: Option<EventId>,
    /// Only those with events. (The server treats *any* value as "yes", so there is no
    /// "without events" option.)
    pub has_events: bool,
    pub status: Vec<Status>,
    pub context_edit: bool,
}

impl LinkedQuery {
    pub(crate) fn to_query(&self) -> Query {
        let mut q = Query::new();
        q.opt("page", self.page)
            .opt("per_page", self.per_page)
            .opt("search", self.search.as_deref())
            .opt("event", self.event)
            .list("status", &self.status);
        if self.has_events {
            q.push("has_events", "1");
        }
        if self.context_edit {
            q.push("context", "edit");
        }
        q
    }
}

/// How to delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeleteMode {
    /// Move to trash (410 if already trashed; 501 if the site disabled trash).
    #[default]
    Trash,
    /// Permanently delete.
    Force,
}

crate::string_enum! {
    pub enum OpenClosed {
        Open = "open",
        Closed = "closed",
    }
}

fn ser_mysql<S: Serializer>(v: &Option<civil::DateTime>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(dt) => s.collect_str(&dt.strftime("%Y-%m-%d %H:%M:%S")),
        None => s.serialize_none(),
    }
}

fn ser_rfc3339ish<S: Serializer>(v: &Option<DateArg>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        // The server pattern needs a `T`/space separator and an optional offset; DateArg's
        // Display satisfies it for every variant except `Day` and `Expr`.
        Some(DateArg::Day(d)) => s.collect_str(&d.at(0, 0, 0, 0).strftime("%Y-%m-%dT%H:%M:%S")),
        Some(other) => s.collect_str(other),
        None => s.serialize_none(),
    }
}

/// Core post fields accepted by every tec/v1 write.
///
/// **Update caveat:** when omitted, the server writes `comment_status=open`, `ping_status=open`,
/// `format=standard` and `sticky=false` (which un-sticks events). Set them explicitly to keep
/// existing values. `title`, `content` and `excerpt` are run through `sanitize_text_field`
/// server-side, which **strips HTML and newlines**.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PostFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// Defaults to `publish` on create (scaled back to `draft`/`pending` without permission).
    /// Left unchanged on update when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<UserId>,
    /// Post publish date.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "ser_rfc3339ish"
    )]
    pub date: Option<DateArg>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "ser_rfc3339ish"
    )]
    pub date_gmt: Option<DateArg>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_status: Option<OpenClosed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ping_status: Option<OpenClosed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sticky: Option<bool>,
}

/// Event fields for create/update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct EventFields {
    #[serde(flatten)]
    pub post: PostFields,
    /// Event-local wall-clock start. Takes precedence over `start_date_utc`.
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "ser_mysql")]
    pub start_date: Option<civil::DateTime>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "ser_mysql")]
    pub start_date_utc: Option<civil::DateTime>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "ser_mysql")]
    pub end_date: Option<civil::DateTime>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "ser_mysql")]
    pub end_date_utc: Option<civil::DateTime>,
    /// IANA zone; `""` means the site zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Seconds. With a start and no end, the end is derived from it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
    /// Note: `Some(false)` does **not** clear all-day on update (server quirk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_day: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub featured: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    #[serde(rename = "tribe_events_cat", skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<TermId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<TermId>>,
    /// Unreadable or invalid ids are silently skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organizers: Option<Vec<OrganizerId>>,
    /// Only the first is used unless the site allows multiple venues.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venues: Option<Vec<VenueId>>,
}

/// When a new event ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventEnd {
    /// Event-local wall-clock end.
    At(civil::DateTime),
    /// Length in seconds.
    After(u64),
}

/// Body for `POST /events`. The server needs a title and an end (explicit or via duration);
/// an end before the start is silently dropped and the create fails with 500.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEvent {
    pub fields: EventFields,
}

impl NewEvent {
    pub fn new(title: impl Into<String>, start: civil::DateTime, end: EventEnd) -> Self {
        let mut fields = EventFields {
            start_date: Some(start),
            ..Default::default()
        };
        fields.post.title = Some(title.into());
        match end {
            EventEnd::At(e) => fields.end_date = Some(e),
            EventEnd::After(secs) => fields.duration = Some(secs),
        }
        Self { fields }
    }
}

impl Serialize for NewEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.fields.serialize(s)
    }
}

/// Venue fields for create/update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct VenueFields {
    #[serde(flatten)]
    pub post: PostFields,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_province: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub province: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// Cannot be cleared: the server rejects `""`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<Url>,
}

/// Body for `POST /venues` (title required).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewVenue {
    pub fields: VenueFields,
}

impl NewVenue {
    pub fn new(title: impl Into<String>) -> Self {
        let mut fields = VenueFields::default();
        fields.post.title = Some(title.into());
        Self { fields }
    }
}

impl Serialize for NewVenue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.fields.serialize(s)
    }
}

/// Organizer fields for create/update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OrganizerFields {
    #[serde(flatten)]
    pub post: PostFields,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// Cannot be cleared: the server rejects `""`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<Url>,
    /// A single address (validated with `is_email`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// Body for `POST /organizers` (title required).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOrganizer {
    pub fields: OrganizerFields,
}

impl NewOrganizer {
    pub fn new(title: impl Into<String>) -> Self {
        let mut fields = OrganizerFields::default();
        fields.post.title = Some(title.into());
        Self { fields }
    }
}

impl Serialize for NewOrganizer {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.fields.serialize(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_uses_brackets_and_01() {
        let q = EventQuery {
            venues: vec![VenueId(5)],
            featured: Some(false),
            order: Some(Order::Desc),
            ..Default::default()
        }
        .to_query();
        assert_eq!(
            q.pairs(),
            &[
                ("venue[]".into(), "5".into()),
                ("featured".into(), "0".into()),
                ("order".into(), "DESC".into()),
            ]
        );
    }

    #[test]
    fn new_event_body() {
        let e = NewEvent::new(
            "Gala",
            civil::date(2026, 12, 1).at(19, 0, 0, 0),
            EventEnd::After(3 * 3600),
        );
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"title": "Gala", "start_date": "2026-12-01 19:00:00", "duration": 10800})
        );
    }
}
