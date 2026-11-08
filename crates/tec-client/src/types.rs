//! Identifier newtypes and enums shared by both APIs.

use serde::{Deserialize, Deserializer, Serialize};

macro_rules! id_type {
    ($($(#[$m:meta])* $name:ident),+ $(,)?) => {$(
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl $name {
            pub const fn get(self) -> u64 { self.0 }
        }

        impl From<u64> for $name {
            fn from(v: u64) -> Self { Self(v) }
        }

        impl From<$name> for u64 {
            fn from(v: $name) -> u64 { v.0 }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(&self.0, f)
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = ::std::num::ParseIntError;
            fn from_str(s: &str) -> Result<Self, Self::Err> { s.parse().map(Self) }
        }

        /// Accepts `12` and `"12"` (PHP emits both).
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                crate::wire::u64_lenient(d).map(Self)
            }
        }
    )+};
}

id_type!(
    /// A `tribe_events` post id.
    EventId,
    /// A `tribe_venue` post id.
    VenueId,
    /// A `tribe_organizer` post id.
    OrganizerId,
    /// A taxonomy term id (category or tag).
    TermId,
    /// A WordPress user id.
    UserId,
    /// A media attachment id.
    AttachmentId,
);

crate::string_enum! {
    /// WordPress post status.
    pub enum PostStatus {
        Publish = "publish",
        Future = "future",
        Draft = "draft",
        Pending = "pending",
        Private = "private",
        Trash = "trash",
        AutoDraft = "auto-draft",
        Inherit = "inherit",
    }
}

crate::string_enum! {
    /// Taxonomies the calendar uses.
    pub enum Taxonomy {
        /// Event categories.
        EventCategory = "tribe_events_cat",
        /// WordPress tags (shared with blog posts).
        PostTag = "post_tag",
    }
}

crate::string_enum! {
    /// Post types the calendar uses.
    pub enum PostType {
        Event = "tribe_events",
        Venue = "tribe_venue",
        Organizer = "tribe_organizer",
    }
}

/// A term reference used in write requests: by id or by slug/name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TermRef {
    Id(TermId),
    /// Slug for filters; name for writes (the server may create the term).
    Slug(String),
}

impl From<TermId> for TermRef {
    fn from(v: TermId) -> Self {
        TermRef::Id(v)
    }
}

impl From<&str> for TermRef {
    fn from(v: &str) -> Self {
        TermRef::Slug(v.to_owned())
    }
}

impl std::fmt::Display for TermRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TermRef::Id(id) => id.fmt(f),
            TermRef::Slug(s) => f.write_str(s),
        }
    }
}

impl Serialize for TermRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            TermRef::Id(id) => s.serialize_u64(id.0),
            TermRef::Slug(slug) => s.serialize_str(slug),
        }
    }
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Order {
    #[serde(rename = "ASC")]
    Asc,
    #[serde(rename = "DESC")]
    Desc,
}

impl Order {
    pub fn as_upper(self) -> &'static str {
        match self {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        }
    }
    pub fn as_lower(self) -> &'static str {
        match self {
            Order::Asc => "asc",
            Order::Desc => "desc",
        }
    }
}

/// A date/time argument for queries and legacy write bodies.
///
/// Sent in a form PHP's `strtotime()` parses unambiguously. Prefer [`DateArg::Zoned`] or
/// [`DateArg::Instant`]: they carry an explicit offset. [`DateArg::Local`] and [`DateArg::Day`]
/// are interpreted by the server (site zone, or UTC in legacy `strict_dates` mode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateArg {
    Instant(jiff::Timestamp),
    Zoned(jiff::Zoned),
    Local(jiff::civil::DateTime),
    Day(jiff::civil::Date),
    /// A raw `strtotime()` expression such as `"now"` or `"+1 week"`.
    Expr(String),
}

impl std::fmt::Display for DateArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DateArg::Instant(t) => f.write_str(&crate::range::php_datetime(
                &t.to_zoned(jiff::tz::TimeZone::UTC),
            )),
            DateArg::Zoned(z) => f.write_str(&crate::range::php_datetime(z)),
            DateArg::Local(dt) => write!(f, "{}", dt.strftime("%Y-%m-%d %H:%M:%S")),
            DateArg::Day(d) => write!(f, "{}", d.strftime("%Y-%m-%d")),
            DateArg::Expr(s) => f.write_str(s),
        }
    }
}

impl Serialize for DateArg {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl From<jiff::Timestamp> for DateArg {
    fn from(v: jiff::Timestamp) -> Self {
        DateArg::Instant(v)
    }
}
impl From<jiff::Zoned> for DateArg {
    fn from(v: jiff::Zoned) -> Self {
        DateArg::Zoned(v)
    }
}
impl From<jiff::civil::DateTime> for DateArg {
    fn from(v: jiff::civil::DateTime) -> Self {
        DateArg::Local(v)
    }
}
impl From<jiff::civil::Date> for DateArg {
    fn from(v: jiff::civil::Date) -> Self {
        DateArg::Day(v)
    }
}

/// Event Tickets status. Older Event Tickets sends a boolean; newer versions send the list of
/// ticket provider slugs (e.g. `["woo"]`) or `false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ticketed {
    /// `false`, or `null` on password-protected events.
    No,
    /// `true` (provider not reported).
    Yes,
    /// Ticket provider slugs.
    Providers(Vec<String>),
}

impl Ticketed {
    pub fn has_tickets(&self) -> bool {
        match self {
            Ticketed::No => false,
            Ticketed::Yes => true,
            Ticketed::Providers(p) => !p.is_empty(),
        }
    }
}

impl Serialize for Ticketed {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Ticketed::No => s.serialize_bool(false),
            Ticketed::Yes => s.serialize_bool(true),
            Ticketed::Providers(p) => p.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for Ticketed {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum In {
            B(bool),
            L(Vec<String>),
            Null(()),
        }
        Ok(match In::deserialize(d)? {
            In::B(true) => Ticketed::Yes,
            In::B(false) | In::Null(()) => Ticketed::No,
            In::L(l) => Ticketed::Providers(l),
        })
    }
}

/// HTML (or HTML-entity-encoded text) exactly as the server rendered it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Html(pub String);

impl Html {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }

    /// Plain text: tags removed (block elements become line breaks), entities decoded,
    /// whitespace collapsed.
    pub fn to_text(&self) -> String {
        crate::text::html_to_text(&self.0)
    }

    /// The links [`Self::to_text`] drops: every `<a href>`, in document order, as
    /// `(label, href)` with entities decoded. The href is verbatim, so it may be relative or a
    /// non-web scheme.
    pub fn links(&self) -> Vec<(String, String)> {
        crate::text::html_links(&self.0)
    }
}

impl From<String> for Html {
    fn from(s: String) -> Self {
        Html(s)
    }
}

impl std::fmt::Display for Html {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A link typed in by a site editor. Usually a valid URL; kept verbatim when it is not
/// (e.g. `www.example.com` without a scheme) rather than dropped.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WebLink {
    Url(url::Url),
    Text(String),
}

impl WebLink {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        if let Ok(u) = url::Url::parse(s)
            && matches!(u.scheme(), "http" | "https" | "mailto" | "tel")
        {
            return Some(WebLink::Url(u));
        }
        // Bare domains: "www.example.com/path".
        if !s.contains(char::is_whitespace)
            && s.contains('.')
            && let Ok(u) = url::Url::parse(&format!("https://{s}"))
        {
            return Some(WebLink::Url(u));
        }
        Some(WebLink::Text(s.to_owned()))
    }

    pub fn url(&self) -> Option<&url::Url> {
        match self {
            WebLink::Url(u) => Some(u),
            WebLink::Text(_) => None,
        }
    }
}

impl std::fmt::Display for WebLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebLink::Url(u) => f.write_str(u.as_str()),
            WebLink::Text(s) => f.write_str(s),
        }
    }
}

impl Serialize for WebLink {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// `host+path?id=N`: identifies a post across sites and Event Aggregator imports.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GlobalId(pub String);

/// A WordPress time zone setting: an IANA name (`America/Chicago`), `UTC`, a `UTC+5.5`-style
/// fixed offset, or empty (use the site zone).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WpTimeZone(pub String);

impl WpTimeZone {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }

    /// Resolve to a `jiff` zone; `None` when empty or unknown.
    pub fn to_time_zone(&self) -> Option<jiff::tz::TimeZone> {
        let s = self.0.trim();
        if s.is_empty() {
            return None;
        }
        if let Some(off) = s.strip_prefix("UTC").filter(|r| !r.is_empty()) {
            let hours: f64 = off.parse().ok()?;
            let secs = (hours * 3600.0).round() as i32;
            return jiff::tz::Offset::from_seconds(secs)
                .ok()
                .map(jiff::tz::TimeZone::fixed);
        }
        jiff::tz::TimeZone::get(s).ok()
    }
}
