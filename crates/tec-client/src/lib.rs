//! Strongly typed async client for [The Events Calendar](https://theeventscalendar.com/)
//! WordPress REST APIs.
//!
//! * [`tribe`]: the legacy `tribe/events/v1` API (every TEC site; events, venues, organizers,
//!   categories, tags; read and write).
//! * [`tec`]: the newer, experimental `tec/v1` API (TEC 6.15+; events, venues, organizers).
//!
//! ```no_run
//! use futures::TryStreamExt;
//! use tec_client::{Bound, Client, CrawlOptions, DateRange, TlsRoots};
//!
//! # async fn run() -> tec_client::Result<()> {
//! let client = Client::builder("https://example.org")?
//!     .tls_roots(TlsRoots::Bundled)
//!     .discover()
//!     .await?;
//!
//! let range = DateRange::new(Bound::Week, Bound::Month);
//! let mut events = client.tribe().events().crawl(&range, Default::default(), CrawlOptions::default())?;
//! while let Some(event) = events.try_next().await? {
//!     println!("{} {}", event.id, event.title);
//! }
//! # Ok(()) }
//! ```

pub mod client;
pub mod error;
pub mod paginate;
pub mod permalink;
pub mod range;
pub mod recurrence;
pub mod series;
pub mod site;
pub mod summary;
pub mod tec;
pub mod tls;
pub mod tribe;
pub mod types;

mod query;
mod text;
mod wire;

pub use client::{Auth, Client, ClientBuilder, RestRoot, RetryPolicy};
pub use error::{ApiError, Error, ErrorBody, Result, WpError};
pub use paginate::{CrawlOptions, Page};
pub use permalink::{EventPermalink, SeriesSlug};
pub use range::{Bound, DateRange, RangeMode, ResolvedRange};
pub use recurrence::{Confidence, InferOptions, InferredRecurrence, RRule};
pub use site::SiteInfo;
pub use series::{GroupOptions, Series, SeriesIndex, SeriesKey, SeriesMember};
pub use summary::{EventSummary, TermSummary, VenueSummary};
pub use tls::TlsRoots;
pub use types::{
    AttachmentId, DateArg, EventId, Order, OrganizerId, PostStatus, PostType, Taxonomy, TermId,
    TermRef, Ticketed, UserId, VenueId,
};

/// Re-exported so callers can name time types without a direct dependency.
pub use jiff;
