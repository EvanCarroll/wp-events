//! Crawl a site running The Events Calendar and print one JSON object per line.
//!
//! ```text
//! tec-crawl --site https://example.org --start week --end month
//! tec-crawl --site https://example.org --api tec --resource venues
//! ```

// Doc comments double as `--help` text, where markdown link syntax would show verbatim.
#![allow(rustdoc::bare_urls)]

mod error;

use std::io::Write;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use std::future::ready;

use futures::stream::{BoxStream, StreamExt, TryStreamExt};
use serde::Serialize;
use tec_client::jiff::civil::Weekday;
use tec_client::jiff::tz::TimeZone;
use tec_client::{
    Auth, Bound, Client, CrawlOptions, DateRange, EventSummary, GroupOptions, InferOptions,
    InferredRecurrence, RangeMode, Series, SeriesIndex, SeriesMember, TermRef, TlsRoots,
};

use crate::error::Error;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Api {
    /// Legacy tribe/events/v1 (works on virtually every site).
    Tribe,
    /// Experimental tec/v1 (TEC 6.15+).
    Tec,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Resource {
    Events,
    Venues,
    Organizers,
    /// Legacy API only.
    Categories,
    /// Legacy API only.
    Tags,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Tls {
    /// Operating system trust store.
    System,
    /// Mozilla roots compiled into this binary.
    Bundled,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Mode {
    /// Events that start inside the window.
    StartsWithin,
    /// Events with any part inside the window (includes ones already running).
    Overlaps,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    /// One concise record per event occurrence: decoded title, zoned start/end, venue,
    /// organizers, cost, url. Venues/organizers/categories/tags are always printed in full.
    Compact,
    /// Every field the API returns, normalised.
    Full,
    /// One record per series (recurring, plus repeating one-off posts unless
    /// `--no-implied`) with its occurrences and an inferred RRULE.
    Series,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum WeekStart {
    Monday,
    Sunday,
    Saturday,
}

/// Crawl The Events Calendar REST APIs and emit JSON lines on stdout.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// Site home URL, e.g. https://example.org
    #[arg(long, env = "TEC_SITE")]
    site: String,

    #[arg(long, value_enum, default_value_t = Api::Tribe)]
    api: Api,

    #[arg(long, value_enum, default_value_t = Resource::Events)]
    resource: Resource,

    /// Output shape for events.
    #[arg(long, value_enum, default_value_t = Format::Compact)]
    format: Format,

    /// With `--format series`: don't group repeating one-off posts into implied series.
    #[arg(long)]
    no_implied: bool,

    /// Window start: today, week, month, year, all, YYYY-MM-DD, or an RFC 3339 timestamp.
    #[arg(long, default_value = "today")]
    start: Bound,

    /// Window end: today, week, month, year, all, YYYY-MM-DD (inclusive), or a timestamp.
    #[arg(long, default_value = "all")]
    end: Bound,

    #[arg(long, value_enum, default_value_t = Mode::StartsWithin)]
    mode: Mode,

    /// Time zone for aligning today/week/month/year (IANA name). Defaults to the system zone;
    /// use the site's zone for exact calendar boundaries.
    #[arg(long)]
    tz: Option<String>,

    #[arg(long, value_enum, default_value_t = WeekStart::Monday)]
    week_start: WeekStart,

    /// Parallel page requests.
    #[arg(long, default_value_t = 4)]
    concurrency: usize,

    /// Items per page (legacy API caps at 50).
    #[arg(long, default_value_t = 50)]
    per_page: u32,

    #[arg(long)]
    max_pages: Option<u32>,

    /// Keep duplicates that appear when data shifts between page requests.
    #[arg(long)]
    no_dedupe: bool,

    #[arg(long, value_enum, default_value_t = Tls::System)]
    tls: Tls,

    /// Skip REST root discovery and assume {site}/wp-json/.
    #[arg(long)]
    no_discover: bool,

    /// Application Password user (for drafts/private items).
    #[arg(long, env = "TEC_USER", requires = "app_password")]
    user: Option<String>,

    #[arg(long, env = "TEC_APP_PASSWORD", hide_env_values = true)]
    app_password: Option<String>,

    /// Free-text search filter.
    #[arg(long)]
    search: Option<String>,

    /// Event category id or slug (repeatable; legacy API only).
    #[arg(long = "category")]
    categories: Vec<String>,

    /// Only featured events.
    #[arg(long)]
    featured: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if let Err(e) = run(args).await {
        eprintln!("error: {e}");
        let mut src = std::error::Error::source(&e);
        while let Some(s) = src {
            eprintln!("  caused by: {s}");
            src = s.source();
        }
        std::process::exit(1);
    }
}

async fn run(args: Args) -> Result<(), Error> {
    let tz = match &args.tz {
        Some(name) => TimeZone::get(name).map_err(|e| Error::TimeZone(name.clone(), e))?,
        None => TimeZone::system(),
    };
    let range = DateRange::new(args.start.clone(), args.end.clone())
        .time_zone(tz.clone())
        .week_start(match args.week_start {
            WeekStart::Monday => Weekday::Monday,
            WeekStart::Sunday => Weekday::Sunday,
            WeekStart::Saturday => Weekday::Saturday,
        })
        .mode(match args.mode {
            Mode::StartsWithin => RangeMode::StartsWithin,
            Mode::Overlaps => RangeMode::Overlaps,
        });
    let resolved = range.resolve()?;
    if matches!(args.resource, Resource::Events) {
        eprintln!(
            "window: {} .. {} ({:?})",
            resolved
                .start
                .as_ref()
                .map_or("unbounded".into(), ToString::to_string),
            resolved
                .end
                .as_ref()
                .map_or("unbounded".into(), ToString::to_string),
            resolved.mode,
        );
    }

    let mut builder = Client::builder(&args.site)?.tls_roots(match args.tls {
        Tls::System => TlsRoots::System,
        Tls::Bundled => TlsRoots::Bundled,
    });
    if let (Some(user), Some(pw)) = (&args.user, &args.app_password) {
        builder = builder.auth(Auth::ApplicationPassword {
            username: user.clone(),
            password: pw.clone(),
        });
    }
    let client = if args.no_discover {
        builder.build()?
    } else {
        builder.discover().await?
    };
    eprintln!("rest root: {:?}", client.rest_root());

    let opts = CrawlOptions {
        per_page: args.per_page,
        concurrency: args.concurrency,
        max_pages: args.max_pages,
        dedupe: !args.no_dedupe,
    };
    let featured = args.featured.then_some(true);

    let stream: BoxStream<'static, tec_client::Result<serde_json::Value>> =
        match (args.api, args.resource) {
            (Api::Tribe, Resource::Events) => {
                let base = tec_client::tribe::params::EventQuery {
                    search: args.search.clone(),
                    categories: args.categories.iter().map(|c| parse_term(c)).collect(),
                    featured,
                    ..Default::default()
                };
                let events = client
                    .tribe()
                    .events()
                    .crawl_resolved(&resolved, base, opts);
                match args.format {
                    Format::Full => to_json(events),
                    Format::Series => series_json(events, tz.clone(), !args.no_implied),
                    Format::Compact => {
                        let tz = tz.clone();
                        to_json(
                            events
                                .map_ok(move |e| EventSummary::from_tribe(&e, &tz))
                                .boxed(),
                        )
                    }
                }
            }
            (Api::Tribe, Resource::Venues) => {
                to_json(client.tribe().venues().crawl(linked_tribe(&args), opts))
            }
            (Api::Tribe, Resource::Organizers) => {
                to_json(client.tribe().organizers().crawl(linked_tribe(&args), opts))
            }
            (Api::Tribe, Resource::Categories) => {
                to_json(client.tribe().categories().crawl(term_query(&args), opts))
            }
            (Api::Tribe, Resource::Tags) => {
                to_json(client.tribe().tags().crawl(term_query(&args), opts))
            }
            (Api::Tec, Resource::Events) => {
                if !args.categories.is_empty() {
                    return Err(Error::Usage(
                        "--category is only supported with --api tribe".into(),
                    ));
                }
                let base = tec_client::tec::params::EventQuery {
                    search: args.search.clone(),
                    featured,
                    ..Default::default()
                };
                let events = client.tec().events().crawl_resolved(&resolved, base, opts);
                match args.format {
                    Format::Full => to_json(events),
                    Format::Series => series_json(
                        events
                            .try_filter_map(|e| ready(Ok(e.into_visible())))
                            .boxed(),
                        tz.clone(),
                        !args.no_implied,
                    ),
                    Format::Compact => {
                        let tz = tz.clone();
                        // Password-protected events have no usable details; skip them.
                        to_json(
                            events
                                .try_filter_map(move |e| ready(Ok(EventSummary::from_tec(&e, &tz))))
                                .boxed(),
                        )
                    }
                }
            }
            (Api::Tec, Resource::Venues) => {
                to_json(client.tec().venues().crawl_all(linked_tec(&args), opts))
            }
            (Api::Tec, Resource::Organizers) => {
                to_json(client.tec().organizers().crawl_all(linked_tec(&args), opts))
            }
            (Api::Tec, Resource::Categories | Resource::Tags) => {
                return Err(Error::Usage(
                    "tec/v1 has no category or tag endpoints; use --api tribe".into(),
                ));
            }
        };

    let started = Instant::now();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let mut count = 0u64;
    let mut stream = stream;
    while let Some(item) = stream.next().await {
        let item = item?;
        serde_json::to_writer(&mut out, &item)?;
        out.write_all(b"\n")?;
        count += 1;
    }
    out.flush()?;
    eprintln!("{count} items in {:.1?}", started.elapsed());
    Ok(())
}

fn parse_term(s: &str) -> TermRef {
    s.parse::<u64>()
        .map(|id| TermRef::Id(id.into()))
        .unwrap_or_else(|_| TermRef::Slug(s.to_owned()))
}

fn linked_tribe(args: &Args) -> tec_client::tribe::params::LinkedQuery {
    tec_client::tribe::params::LinkedQuery {
        search: args.search.clone(),
        ..Default::default()
    }
}

fn linked_tec(args: &Args) -> tec_client::tec::params::LinkedQuery {
    tec_client::tec::params::LinkedQuery {
        search: args.search.clone(),
        ..Default::default()
    }
}

fn term_query(args: &Args) -> tec_client::tribe::params::TermQuery {
    tec_client::tribe::params::TermQuery {
        search: args.search.clone(),
        hide_empty: Some(false),
        ..Default::default()
    }
}

/// Collect every event, group into series, infer a rule per series.
fn series_json<E>(
    events: BoxStream<'static, tec_client::Result<E>>,
    tz: TimeZone,
    implied: bool,
) -> BoxStream<'static, tec_client::Result<serde_json::Value>>
where
    E: SeriesMember + Send + 'static,
{
    #[derive(Serialize)]
    struct Out<'a> {
        #[serde(flatten)]
        series: &'a Series,
        recurrence: Option<InferredRecurrence>,
        ical: Option<String>,
    }
    futures::stream::once(async move {
        let all: Vec<E> = events.try_collect().await?;
        let opts = GroupOptions {
            site_tz: tz,
            implied,
            ..Default::default()
        };
        let index = SeriesIndex::build(&all, &opts);
        index
            .series()
            .iter()
            .map(|s| {
                let recurrence = s.infer_recurrence(InferOptions::default());
                let ical = recurrence.as_ref().map(InferredRecurrence::to_ical);
                serde_json::to_value(Out {
                    series: s,
                    recurrence,
                    ical,
                })
                .map_err(|e| tec_client::Error::Config(format!("encode: {e}")))
            })
            .collect::<tec_client::Result<Vec<_>>>()
    })
    .map_ok(|v| futures::stream::iter(v.into_iter().map(Ok)))
    .try_flatten()
    .boxed()
}

fn to_json<T: Serialize + Send + 'static>(
    s: BoxStream<'static, tec_client::Result<T>>,
) -> BoxStream<'static, tec_client::Result<serde_json::Value>> {
    s.map(|r| {
        r.and_then(|v| {
            serde_json::to_value(v).map_err(|e| tec_client::Error::Config(format!("encode: {e}")))
        })
    })
    .boxed()
}
