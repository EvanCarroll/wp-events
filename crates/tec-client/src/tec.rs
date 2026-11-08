//! The newer `tec/v1` API (The Events Calendar 6.15+).
//!
//! Every endpoint except `/docs` is flagged experimental by the plugin and requires an
//! acknowledgement header, which this client sends for you. Expect it to change between
//! plugin releases.

pub mod models;
pub mod params;

use std::fmt::Display;
use std::hash::Hash;
use std::marker::PhantomData;

use std::future::ready;

use futures::stream::{BoxStream, StreamExt, TryStreamExt};
use http::Method;
use http::header::{HeaderName, HeaderValue};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::client::{Client, RawResponse};
use crate::error::{Error, Result};
use crate::paginate::{self, CrawlOptions, Page};
use crate::query::Query;
use crate::range::{DateRange, RangeMode, ResolvedRange};
use crate::types::{DateArg, EventId, OrganizerId, VenueId};

use models::{Entity, Event, Organizer, Protectable, Venue};
use params::{
    DeleteMode, EventFields, EventQuery, LinkedQuery, NewEvent, NewOrganizer, NewVenue,
    OrganizerFields, VenueFields,
};

const NS: [&str; 2] = ["tec", "v1"];

/// The exact sentence the server requires in `X-TEC-EEA`.
pub const EXPERIMENTAL_ACK: &str = "I understand that this endpoint is experimental and may change in a future release without maintaining backward compatibility. I also understand that I am using this endpoint at my own risk, while support is not provided for it.";

/// Entry point for `tec/v1`; get it from [`Client::tec`].
#[derive(Debug, Clone)]
pub struct TecApi {
    client: Client,
}

impl TecApi {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    async fn call(
        &self,
        method: Method,
        tail: &[&str],
        query: &Query,
        body: Option<serde_json::Value>,
    ) -> Result<RawResponse> {
        let segs: Vec<&str> = NS.iter().copied().chain(tail.iter().copied()).collect();
        let ack = [(
            HeaderName::from_static("x-tec-eea"),
            HeaderValue::from_static(EXPERIMENTAL_ACK),
        )];
        self.client
            .send(method, &segs, query, body.as_ref(), &ack)
            .await
    }

    /// The OpenAPI 3 document (not experimental). Several schemas disagree with real responses.
    pub async fn docs(&self) -> Result<serde_json::Value> {
        self.call(Method::GET, &["docs"], &Query::new(), None)
            .await?
            .json()
    }

    pub fn events(&self) -> Collection<EventKind> {
        Collection::new(self.clone())
    }

    pub fn venues(&self) -> Collection<VenueKind> {
        Collection::new(self.clone())
    }

    pub fn organizers(&self) -> Collection<OrganizerKind> {
        Collection::new(self.clone())
    }
}

mod private {
    use crate::query::Query;

    /// Query encoding, kept crate-private so the trait is sealed.
    pub trait QueryImpl {
        fn to_query(&self) -> Query;
        fn set_page(&mut self, page: u32, per_page: u32);
        fn page(&self) -> u32;
    }
}

use private::QueryImpl;

/// Query types accepted by [`Collection`]: [`EventQuery`] and [`LinkedQuery`]. Sealed.
pub trait ToQuery: QueryImpl + Clone + Send + Sync + 'static {}
impl ToQuery for EventQuery {}
impl ToQuery for LinkedQuery {}

impl QueryImpl for EventQuery {
    fn to_query(&self) -> Query {
        EventQuery::to_query(self)
    }
    fn set_page(&mut self, page: u32, per_page: u32) {
        self.page = Some(page);
        self.per_page = Some(per_page);
    }
    fn page(&self) -> u32 {
        self.page.unwrap_or(1)
    }
}

impl QueryImpl for LinkedQuery {
    fn to_query(&self) -> Query {
        LinkedQuery::to_query(self)
    }
    fn set_page(&mut self, page: u32, per_page: u32) {
        self.page = Some(page);
        self.per_page = Some(per_page);
    }
    fn page(&self) -> u32 {
        self.page.unwrap_or(1)
    }
}

/// Distinguishes the three tec/v1 entity collections.
pub trait TecKind: Send + Sync + 'static {
    const PATH: &'static str;
    type Id: Display + Copy + Eq + Hash + Send + Sync + 'static;
    type Item: DeserializeOwned + Entity + Send + 'static;
    type Query: ToQuery;
    type New: Serialize + Sync;
    type Update: Serialize + Sync;
}

#[derive(Debug, Clone, Copy)]
pub enum EventKind {}
impl TecKind for EventKind {
    const PATH: &'static str = "events";
    type Id = EventId;
    type Item = Event;
    type Query = EventQuery;
    type New = NewEvent;
    type Update = EventFields;
}

#[derive(Debug, Clone, Copy)]
pub enum VenueKind {}
impl TecKind for VenueKind {
    const PATH: &'static str = "venues";
    type Id = VenueId;
    type Item = Venue;
    type Query = LinkedQuery;
    type New = NewVenue;
    type Update = VenueFields;
}

#[derive(Debug, Clone, Copy)]
pub enum OrganizerKind {}
impl TecKind for OrganizerKind {
    const PATH: &'static str = "organizers";
    type Id = OrganizerId;
    type Item = Organizer;
    type Query = LinkedQuery;
    type New = NewOrganizer;
    type Update = OrganizerFields;
}

/// `/events`, `/venues` or `/organizers`.
#[derive(Debug)]
pub struct Collection<K> {
    api: TecApi,
    _k: PhantomData<K>,
}

impl<K> Clone for Collection<K> {
    fn clone(&self) -> Self {
        Self {
            api: self.api.clone(),
            _k: PhantomData,
        }
    }
}

fn to_body<B: Serialize>(b: &B) -> Result<serde_json::Value> {
    serde_json::to_value(b).map_err(|e| Error::Config(format!("cannot serialise body: {e}")))
}

impl<K: TecKind> Collection<K> {
    fn new(api: TecApi) -> Self {
        Self {
            api,
            _k: PhantomData,
        }
    }

    /// One page. Totals come from `X-WP-Total`/`X-WP-TotalPages`.
    pub async fn list(&self, query: &K::Query) -> Result<Page<Protectable<K::Item>>> {
        let raw = self
            .api
            .call(Method::GET, &[K::PATH], &query.to_query(), None)
            .await?;
        let items: Vec<Protectable<K::Item>> = raw.json()?;
        Ok(Page {
            total: raw.header_u64("x-wp-total").unwrap_or(items.len() as u64),
            total_pages: raw
                .header_u64("x-wp-totalpages")
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(1),
            page: query.page(),
            items,
        })
    }

    /// A single item. **Nonexistent ids return 401/403, not 404.**
    pub async fn get(&self, id: K::Id) -> Result<Protectable<K::Item>> {
        self.api
            .call(
                Method::GET,
                &[K::PATH, &id.to_string()],
                &Query::new(),
                None,
            )
            .await?
            .json()
    }

    /// A password-protected item, unlocked.
    pub async fn get_with_password(
        &self,
        id: K::Id,
        password: &str,
    ) -> Result<Protectable<K::Item>> {
        let mut q = Query::new();
        q.push("password", password);
        self.api
            .call(Method::GET, &[K::PATH, &id.to_string()], &q, None)
            .await?
            .json()
    }

    /// Create (201).
    pub async fn create(&self, new: &K::New) -> Result<Protectable<K::Item>> {
        self.api
            .call(Method::POST, &[K::PATH], &Query::new(), Some(to_body(new)?))
            .await?
            .json()
    }

    /// Partial update (PATCH). See [`params::PostFields`] for fields the server resets.
    pub async fn update(&self, id: K::Id, update: &K::Update) -> Result<Protectable<K::Item>> {
        self.api
            .call(
                Method::PATCH,
                &[K::PATH, &id.to_string()],
                &Query::new(),
                Some(to_body(update)?),
            )
            .await?
            .json()
    }

    /// Trash or permanently delete. The server returns no entity.
    pub async fn delete(&self, id: K::Id, mode: DeleteMode) -> Result<()> {
        // `force` goes in a JSON body: as a query string, `force=false` would be cast to true.
        let body = serde_json::json!({ "force": mode == DeleteMode::Force });
        self.api
            .call(
                Method::DELETE,
                &[K::PATH, &id.to_string()],
                &Query::new(),
                Some(body),
            )
            .await?;
        Ok(())
    }

    /// Stream every page matching `query`, up to `opts.concurrency` requests at once.
    pub fn pages(
        &self,
        query: K::Query,
        opts: CrawlOptions,
    ) -> BoxStream<'static, Result<Page<Protectable<K::Item>>>> {
        let this = self.clone();
        paginate::pages(opts, move |n| {
            let this = this.clone();
            let mut q = query.clone();
            q.set_page(n, opts.per_page);
            async move { this.list(&q).await }
        })
    }

    /// Stream every item matching `query`.
    pub fn crawl_all(
        &self,
        query: K::Query,
        opts: CrawlOptions,
    ) -> BoxStream<'static, Result<Protectable<K::Item>>> {
        paginate::items(
            self.pages(query, opts),
            opts.dedupe,
            |p: &Protectable<K::Item>| p.row_key(),
        )
    }
}

impl Collection<EventKind> {
    /// Stream every event in a calendar-aligned window. Other filters in `base` are kept;
    /// its date fields are replaced.
    ///
    /// **Recurring events (Events Calendar Pro) are unreliable here.** The server checks the
    /// window against the series parent's *first* occurrence, so a series that began before
    /// the window is dropped entirely, even when it has occurrences inside it. This client
    /// removes occurrences the server wrongly includes, but it cannot recover dropped ones.
    /// Use the legacy API (`client.tribe().events().crawl`) for complete calendars.
    pub fn crawl(
        &self,
        range: &DateRange,
        base: EventQuery,
        opts: CrawlOptions,
    ) -> Result<BoxStream<'static, Result<Protectable<Event>>>> {
        let resolved = range.resolve()?;
        Ok(self.crawl_resolved(&resolved, base, opts))
    }

    pub fn crawl_resolved(
        &self,
        range: &ResolvedRange,
        mut base: EventQuery,
        opts: CrawlOptions,
    ) -> BoxStream<'static, Result<Protectable<Event>>> {
        apply_range(&mut base, range);
        let range = range.clone();
        self.crawl_all(base, opts)
            .try_filter(move |item| ready(in_window(item, &range)))
            .boxed()
    }
}

/// The server filters recurring series by the parent's first occurrence and then returns
/// *every* occurrence, including ones outside the window. Re-check each row locally.
fn in_window(item: &Protectable<Event>, r: &ResolvedRange) -> bool {
    let Protectable::Visible(e) = item else {
        return true;
    };
    let (Some(start), end) = (e.start_instant(), e.end_instant()) else {
        return true;
    };
    let lower_ok = match (&r.start, r.mode) {
        (None, _) => true,
        (Some(s), RangeMode::StartsWithin) => start >= s.timestamp(),
        (Some(s), RangeMode::Overlaps) => end.unwrap_or(start) > s.timestamp(),
    };
    let upper_ok = r.end.as_ref().is_none_or(|e| start < e.timestamp());
    lower_ok && upper_ok
}

fn apply_range(q: &mut EventQuery, r: &ResolvedRange) {
    q.start_date = None;
    q.end_date = None;
    q.starts_before = None;
    q.starts_after = None;
    q.ends_before = None;
    q.ends_after = None;
    q.starts_before = r.end.clone().map(DateArg::Zoned);
    match r.mode {
        RangeMode::StartsWithin => q.start_date = r.start.clone().map(DateArg::Zoned),
        RangeMode::Overlaps => q.ends_after = r.start.clone().map(DateArg::Zoned),
    }
}
