//! The legacy `tribe/events/v1` API.
//!
//! Present on essentially every site running The Events Calendar (enabled unless the site
//! sets the `rest-v1-disabled` option, in which case every call fails with `rest_no_route`).
//! Reads of published content are anonymous; writes need [`crate::Auth`].

pub mod models;
pub mod params;

use std::fmt::Display;
use std::marker::PhantomData;

use futures::stream::BoxStream;
use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::paginate::{self, CrawlOptions, Page};
use crate::query::Query;
use crate::range::{DateRange, RangeMode, ResolvedRange};
use crate::types::{DateArg, EventId, OrganizerId, TermId, VenueId};

use models::{ArchiveWire, Event, Organizer, Term, Venue};
use params::{
    EventQuery, EventUpdate, LinkedQuery, NewEvent, NewOrganizer, NewTerm, NewVenue,
    OrganizerUpdate, TermQuery, TermUpdate, VenueUpdate,
};

const NS: [&str; 3] = ["tribe", "events", "v1"];

/// Far bounds used for "unbounded" sides of a window: the legacy API otherwise applies its
/// default `[yesterday, +24 months]` window.
const FAR_PAST: &str = "1900-01-01T00:00:00+00:00";
const FAR_FUTURE: &str = "2999-12-31T23:59:59+00:00";

/// Entry point for `tribe/events/v1`; get it from [`Client::tribe`].
#[derive(Debug, Clone)]
pub struct TribeApi {
    client: Client,
}

impl TribeApi {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        tail: &[&str],
        query: &Query,
        body: Option<serde_json::Value>,
    ) -> Result<(T, crate::client::RawResponse)> {
        let segs: Vec<&str> = NS.iter().copied().chain(tail.iter().copied()).collect();
        let raw = self
            .client
            .send(method, &segs, query, body.as_ref(), &[])
            .await?;
        Ok((raw.json()?, raw))
    }

    async fn get_one<T: DeserializeOwned>(&self, tail: &[&str], query: &Query) -> Result<T> {
        Ok(self.call(Method::GET, tail, query, None).await?.0)
    }

    async fn write<T: DeserializeOwned, B: Serialize>(
        &self,
        method: Method,
        tail: &[&str],
        body: &B,
    ) -> Result<T> {
        let body = serde_json::to_value(body)
            .map_err(|e| Error::Config(format!("cannot serialise body: {e}")))?;
        Ok(self.call(method, tail, &Query::new(), Some(body)).await?.0)
    }

    async fn archive<T: DeserializeOwned>(
        &self,
        tail: &[&str],
        query: &Query,
        page: u32,
    ) -> Result<Page<T>> {
        let (wire, _): (ArchiveWire<T>, _) = self.call(Method::GET, tail, query, None).await?;
        Ok(Page {
            items: wire.items,
            page,
            total: wire.total,
            total_pages: u32::try_from(wire.total_pages).unwrap_or(u32::MAX),
        })
    }

    /// The (inaccurate) OpenAPI document the plugin publishes at `/doc`.
    pub async fn doc(&self) -> Result<serde_json::Value> {
        self.get_one(&["doc"], &Query::new()).await
    }

    pub fn events(&self) -> Events {
        Events { api: self.clone() }
    }

    pub fn venues(&self) -> Linked<VenueKind> {
        Linked {
            api: self.clone(),
            _k: PhantomData,
        }
    }

    pub fn organizers(&self) -> Linked<OrganizerKind> {
        Linked {
            api: self.clone(),
            _k: PhantomData,
        }
    }

    pub fn categories(&self) -> Terms<CategoryKind> {
        Terms {
            api: self.clone(),
            _k: PhantomData,
        }
    }

    pub fn tags(&self) -> Terms<TagKind> {
        Terms {
            api: self.clone(),
            _k: PhantomData,
        }
    }
}

/// `/events` endpoints.
#[derive(Debug, Clone)]
pub struct Events {
    api: TribeApi,
}

impl Events {
    /// One page of events.
    pub async fn list(&self, query: &EventQuery) -> Result<Page<Event>> {
        self.api
            .archive(&["events"], &query.to_query(), query.page.unwrap_or(1))
            .await
    }

    /// A single event. Includes `json_ld`.
    pub async fn get(&self, id: EventId) -> Result<Event> {
        self.api
            .get_one(&["events", &id.to_string()], &Query::new())
            .await
    }

    /// A password-protected event.
    pub async fn get_with_password(&self, id: EventId, password: &str) -> Result<Event> {
        let mut q = Query::new();
        q.push("password", password);
        self.api.get_one(&["events", &id.to_string()], &q).await
    }

    /// A single event by slug. An unknown slug is a 400 `rest_invalid_param`, not a 404.
    pub async fn get_by_slug(&self, slug: &str) -> Result<Event> {
        self.api
            .get_one(&["events", "by-slug", slug], &Query::new())
            .await
    }

    /// Create an event (201). Requires `edit_posts` on events.
    pub async fn create(&self, event: &NewEvent) -> Result<Event> {
        self.api.write(Method::POST, &["events"], event).await
    }

    /// Update an event. Read [`EventUpdate`]'s warning about reset fields first.
    pub async fn update(&self, id: EventId, update: &EventUpdate) -> Result<Event> {
        self.api
            .write(Method::POST, &["events", &id.to_string()], update)
            .await
    }

    pub async fn update_by_slug(&self, slug: &str, update: &EventUpdate) -> Result<Event> {
        self.api
            .write(Method::POST, &["events", "by-slug", slug], update)
            .await
    }

    /// Move an event to the trash; returns it with `status: trash`. 410 if already trashed.
    pub async fn delete(&self, id: EventId) -> Result<Event> {
        let (e, _) = self
            .api
            .call(
                Method::DELETE,
                &["events", &id.to_string()],
                &Query::new(),
                None,
            )
            .await?;
        Ok(e)
    }

    pub async fn delete_by_slug(&self, slug: &str) -> Result<Event> {
        let (e, _) = self
            .api
            .call(
                Method::DELETE,
                &["events", "by-slug", slug],
                &Query::new(),
                None,
            )
            .await?;
        Ok(e)
    }

    /// The parent post of a recurring series, from the slug in an occurrence's permalink
    /// (`event.permalink()?.series()?`).
    pub async fn series_parent(&self, series: &crate::permalink::SeriesSlug) -> Result<Event> {
        self.get_by_slug(&series.0).await
    }

    /// Every occurrence of a recurring series, past and future, by its parent post id.
    /// (The server expands `include=<parent>` to all occurrences.)
    pub async fn series_occurrences(
        &self,
        parent: EventId,
        opts: CrawlOptions,
    ) -> Result<Vec<Event>> {
        use futures::TryStreamExt;
        let query = EventQuery {
            include: vec![parent],
            // A relative bound is needed to lift the server's default date window.
            starts_after: Some(DateArg::Expr(FAR_PAST.into())),
            strict_dates: Some(true),
            ..Default::default()
        };
        paginate::items(self.pages(query, opts), opts.dedupe, |e: &Event| e.id)
            .try_collect()
            .await
    }

    /// Stream every page matching `query`, fetching up to `opts.concurrency` pages at once.
    /// `query.page` and `query.per_page` are overridden.
    pub fn pages(
        &self,
        query: EventQuery,
        opts: CrawlOptions,
    ) -> BoxStream<'static, Result<Page<Event>>> {
        let this = self.clone();
        paginate::pages(opts, move |n| {
            let this = this.clone();
            let mut q = query.clone();
            q.page = Some(n);
            q.per_page = Some(opts.per_page);
            async move { this.list(&q).await }
        })
    }

    /// Stream every event in a calendar-aligned window, e.g.
    /// `DateRange::new(Bound::Week, Bound::Month)`. Other filters in `base` (categories,
    /// venues, search, ...) are kept; its date fields are replaced.
    pub fn crawl(
        &self,
        range: &DateRange,
        base: EventQuery,
        opts: CrawlOptions,
    ) -> Result<BoxStream<'static, Result<Event>>> {
        let resolved = range.resolve()?;
        Ok(self.crawl_resolved(&resolved, base, opts))
    }

    /// Like [`Events::crawl`] with an already-resolved window.
    pub fn crawl_resolved(
        &self,
        range: &ResolvedRange,
        mut base: EventQuery,
        opts: CrawlOptions,
    ) -> BoxStream<'static, Result<Event>> {
        apply_range(&mut base, range);
        paginate::items(self.pages(base, opts), opts.dedupe, |e: &Event| e.id)
    }
}

/// Translate a window into legacy parameters with exact (strict) semantics.
fn apply_range(q: &mut EventQuery, r: &ResolvedRange) {
    q.start_date = None;
    q.end_date = None;
    q.starts_before = None;
    q.starts_after = None;
    q.ends_before = None;
    q.ends_after = None;
    q.strict_dates = Some(true);
    let end = r
        .end
        .clone()
        .map_or_else(|| DateArg::Expr(FAR_FUTURE.into()), DateArg::Zoned);
    // Sending any starts_*/ends_* bound removes the server's default window.
    q.starts_before = Some(end);
    match r.mode {
        RangeMode::StartsWithin => {
            q.start_date = Some(
                r.start
                    .clone()
                    .map_or_else(|| DateArg::Expr(FAR_PAST.into()), DateArg::Zoned),
            );
        }
        RangeMode::Overlaps => {
            q.ends_after = r.start.clone().map(DateArg::Zoned);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Venues and organizers
// ---------------------------------------------------------------------------------------------

/// Distinguishes venues from organizers in [`Linked`].
pub trait LinkedKind: Send + Sync + 'static {
    const PATH: &'static str;
    type Id: Display + Copy + Send + Sync + 'static;
    type Item: DeserializeOwned + Send + 'static;
    type New: Serialize + Sync;
    type Update: Serialize + Sync;
    fn id(item: &Self::Item) -> Self::Id;
}

#[derive(Debug, Clone, Copy)]
pub enum VenueKind {}
impl LinkedKind for VenueKind {
    const PATH: &'static str = "venues";
    type Id = VenueId;
    type Item = Venue;
    type New = NewVenue;
    type Update = VenueUpdate;
    fn id(item: &Venue) -> VenueId {
        item.id
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OrganizerKind {}
impl LinkedKind for OrganizerKind {
    const PATH: &'static str = "organizers";
    type Id = OrganizerId;
    type Item = Organizer;
    type New = NewOrganizer;
    type Update = OrganizerUpdate;
    fn id(item: &Organizer) -> OrganizerId {
        item.id
    }
}

/// `/venues` or `/organizers` endpoints.
#[derive(Debug)]
pub struct Linked<K> {
    api: TribeApi,
    _k: PhantomData<K>,
}

impl<K> Clone for Linked<K> {
    fn clone(&self) -> Self {
        Self {
            api: self.api.clone(),
            _k: PhantomData,
        }
    }
}

impl<K: LinkedKind> Linked<K>
where
    K::Id: std::hash::Hash + Eq,
{
    pub async fn list(&self, query: &LinkedQuery) -> Result<Page<K::Item>> {
        self.api
            .archive(&[K::PATH], &query.to_query(), query.page.unwrap_or(1))
            .await
    }

    /// Unknown ids are a 400 `rest_invalid_param`.
    pub async fn get(&self, id: K::Id) -> Result<K::Item> {
        self.api
            .get_one(&[K::PATH, &id.to_string()], &Query::new())
            .await
    }

    pub async fn get_with_password(&self, id: K::Id, password: &str) -> Result<K::Item> {
        let mut q = Query::new();
        q.push("password", password);
        self.api.get_one(&[K::PATH, &id.to_string()], &q).await
    }

    pub async fn get_by_slug(&self, slug: &str) -> Result<K::Item> {
        self.api
            .get_one(&[K::PATH, "by-slug", slug], &Query::new())
            .await
    }

    pub async fn create(&self, new: &K::New) -> Result<K::Item> {
        self.api.write(Method::POST, &[K::PATH], new).await
    }

    /// See the type-level docs of the update body for fields reset when omitted.
    pub async fn update(&self, id: K::Id, update: &K::Update) -> Result<K::Item> {
        self.api
            .write(Method::POST, &[K::PATH, &id.to_string()], update)
            .await
    }

    pub async fn update_by_slug(&self, slug: &str, update: &K::Update) -> Result<K::Item> {
        self.api
            .write(Method::POST, &[K::PATH, "by-slug", slug], update)
            .await
    }

    /// Move to trash; returns the item. 410 if already trashed.
    pub async fn delete(&self, id: K::Id) -> Result<K::Item> {
        Ok(self
            .api
            .call(
                Method::DELETE,
                &[K::PATH, &id.to_string()],
                &Query::new(),
                None,
            )
            .await?
            .0)
    }

    pub fn pages(
        &self,
        query: LinkedQuery,
        opts: CrawlOptions,
    ) -> BoxStream<'static, Result<Page<K::Item>>> {
        let this = self.clone();
        paginate::pages(opts, move |n| {
            let this = this.clone();
            let mut q = query.clone();
            q.page = Some(n);
            q.per_page = Some(opts.per_page);
            async move { this.list(&q).await }
        })
    }

    /// Stream every item matching `query`.
    pub fn crawl(
        &self,
        query: LinkedQuery,
        opts: CrawlOptions,
    ) -> BoxStream<'static, Result<K::Item>> {
        paginate::items(self.pages(query, opts), opts.dedupe, K::id)
    }
}

// ---------------------------------------------------------------------------------------------
// Categories and tags
// ---------------------------------------------------------------------------------------------

/// Distinguishes categories from tags in [`Terms`].
pub trait TermKind: Send + Sync + 'static {
    const PATH: &'static str;
    const HIERARCHICAL: bool;
}

#[derive(Debug, Clone, Copy)]
pub enum CategoryKind {}
impl TermKind for CategoryKind {
    const PATH: &'static str = "categories";
    const HIERARCHICAL: bool = true;
}

/// Note: `post_tag` is shared with blog posts; counts include all post types.
#[derive(Debug, Clone, Copy)]
pub enum TagKind {}
impl TermKind for TagKind {
    const PATH: &'static str = "tags";
    const HIERARCHICAL: bool = false;
}

/// `/categories` or `/tags` endpoints.
#[derive(Debug)]
pub struct Terms<K> {
    api: TribeApi,
    _k: PhantomData<K>,
}

impl<K> Clone for Terms<K> {
    fn clone(&self) -> Self {
        Self {
            api: self.api.clone(),
            _k: PhantomData,
        }
    }
}

impl<K: TermKind> Terms<K> {
    fn check_parent(parent: Option<TermId>) -> Result<()> {
        if parent.is_some() && !K::HIERARCHICAL {
            return Err(Error::Config(format!(
                "{} are not hierarchical; `parent` is not allowed",
                K::PATH
            )));
        }
        Ok(())
    }

    pub async fn list(&self, query: &TermQuery) -> Result<Page<Term>> {
        Self::check_parent(query.parent)?;
        self.api
            .archive(&[K::PATH], &query.to_query(), query.page.unwrap_or(1))
            .await
    }

    pub async fn get(&self, id: TermId) -> Result<Term> {
        self.api
            .get_one(&[K::PATH, &id.to_string()], &Query::new())
            .await
    }

    /// Requires `edit_posts` on events and `manage_terms` on the taxonomy.
    /// A duplicate name is a 400 `term_exists`.
    pub async fn create(&self, term: &NewTerm) -> Result<Term> {
        Self::check_parent(term.parent)?;
        self.api.write(Method::POST, &[K::PATH], term).await
    }

    pub async fn update(&self, id: TermId, update: &TermUpdate) -> Result<Term> {
        Self::check_parent(update.parent)?;
        self.api
            .write(Method::POST, &[K::PATH, &id.to_string()], update)
            .await
    }

    /// **Permanently** deletes the term; returns its previous data.
    pub async fn delete(&self, id: TermId) -> Result<Term> {
        Ok(self
            .api
            .call(
                Method::DELETE,
                &[K::PATH, &id.to_string()],
                &Query::new(),
                None,
            )
            .await?
            .0)
    }

    pub fn pages(
        &self,
        query: TermQuery,
        opts: CrawlOptions,
    ) -> BoxStream<'static, Result<Page<Term>>> {
        let this = self.clone();
        paginate::pages(opts, move |n| {
            let this = this.clone();
            let mut q = query.clone();
            q.page = Some(n);
            q.per_page = Some(opts.per_page);
            async move { this.list(&q).await }
        })
    }

    /// Stream every term. Set `hide_empty: Some(false)` to include unused terms.
    pub fn crawl(&self, query: TermQuery, opts: CrawlOptions) -> BoxStream<'static, Result<Term>> {
        paginate::items(self.pages(query, opts), opts.dedupe, |t: &Term| t.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Zoned;

    #[test]
    fn range_to_params() {
        let s: Zoned = "2026-11-09T00:00:00-05:00[America/New_York]"
            .parse()
            .unwrap();
        let e: Zoned = "2026-12-01T00:00:00-05:00[America/New_York]"
            .parse()
            .unwrap();
        let mut q = EventQuery::default();
        apply_range(
            &mut q,
            &ResolvedRange {
                start: Some(s.clone()),
                end: Some(e),
                mode: RangeMode::StartsWithin,
            },
        );
        let pairs = q.to_query();
        assert_eq!(
            pairs.pairs(),
            &[
                ("start_date".into(), "2026-11-09T00:00:00-05:00".into()),
                ("starts_before".into(), "2026-12-01T00:00:00-05:00".into()),
                ("strict_dates".into(), "true".into()),
            ]
        );

        let mut q = EventQuery::default();
        apply_range(
            &mut q,
            &ResolvedRange {
                start: Some(s),
                end: None,
                mode: RangeMode::Overlaps,
            },
        );
        assert_eq!(
            q.to_query().pairs(),
            &[
                ("starts_before".into(), FAR_FUTURE.into()),
                ("ends_after".into(), "2026-11-09T00:00:00-05:00".into()),
                ("strict_dates".into(), "true".into()),
            ]
        );
    }
}
