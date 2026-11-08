# wp-events

A Rust client for the REST APIs of [The Events Calendar](https://theeventscalendar.com/) (TEC), the
WordPress events plugin.

| Crate | What it is |
|---|---|
| `crates/tec-client` | Async library: typed models, every endpoint, parallel crawling, TLS choice |
| `crates/tec-crawl` | CLI built on the library: crawls a site and prints JSON lines |

## Install

```toml
[dependencies]
tec-client = "1"
```

```sh
cargo install tec-crawl
```

The wire-level API specifications this client was built from (derived from the plugin source,
TEC 6.18.3.1) are in [`docs/`](docs/): [`spec_tribe_v1.md`](docs/spec_tribe_v1.md) and
[`spec_tec_v1.md`](docs/spec_tec_v1.md).

## APIs covered

| | `tribe/events/v1` (legacy) — `client.tribe()` | `tec/v1` (new) — `client.tec()` |
|---|---|---|
| Availability | Every TEC site (unless disabled) | TEC 6.15+, marked **experimental** |
| Events | list, get, get by slug, create, update, delete, crawl | list, get, create, update (PATCH), delete (trash/force), crawl |
| Venues / organizers | list, get, get by slug, create, update, delete, crawl | list, get, create, update, delete, crawl |
| Categories / tags | list, get, create, update, delete, crawl | — |
| Docs | `doc()` | `docs()` |
| Recurring events (Pro) | one item per occurrence, each with its own id | occurrences share the **parent's id**, and date filters are checked against the series' **first** occurrence (see below) |

**Use the legacy API for crawling calendars.** It is on every site and it lists every occurrence of
a recurring event.

### tec/v1 and recurring events

Verified on iondistrict.com (TEC 6.18 with Events Calendar Pro), using series 60334, which runs
monthly from 2026-07-09 to 2027-04-08:

| `tec/v1` query | Rows returned |
|---|---|
| no date filter | 10 occurrences, **all with `id: 60334`** |
| a July 2026 window | all 10, including ones in 2027 |
| a November 2026 window | **0**, though Nov 12 is an occurrence |

The server checks the window against the parent post's stored dates, which belong to the first
occurrence. If those match, it returns every occurrence; if not, it returns none.
`GET /tec/v1/events/{occurrence id}` returns 404.

How the client handles this:
- De-duplication uses `(id, start)` (`Protectable::key`), so occurrences are not merged.
- Windowed crawls drop occurrences that fall outside the window.
- Series the server dropped **cannot be recovered**. Use the legacy API when completeness matters.

## Quick start

```rust
use futures::TryStreamExt;
use tec_client::{Bound, Client, CrawlOptions, DateRange, TlsRoots};
use tec_client::jiff::tz::TimeZone;

let client = Client::builder("https://example.org")?
    .tls_roots(TlsRoots::System)       // or TlsRoots::Bundled
    .discover().await?;                // finds /wp-json/ or ?rest_route= via the Link header

let range = DateRange::new(Bound::Week, Bound::Month)
    .time_zone(TimeZone::get("America/New_York")?);

let mut events = client.tribe().events()
    .crawl(&range, Default::default(), CrawlOptions { concurrency: 4, ..Default::default() })?;
while let Some(e) = events.try_next().await? {
    println!("{} {:?} {}", e.id, e.start_date, e.title);
}
```

CLI:

```sh
cargo run -p tec-crawl -- --site https://demo.theeventscalendar.com --start week --end month --tz America/Detroit
cargo run -p tec-crawl -- --site https://example.org --resource venues
cargo run -p tec-crawl -- --site https://example.org --api tec --start all --end all --tls bundled
TEC_USER=me TEC_APP_PASSWORD='xxxx xxxx' cargo run -p tec-crawl -- --site https://example.org   # include drafts
```

Events are printed in **compact** form by default (`tec_client::EventSummary`): `id`, `series_id`
(tec/v1 recurring only), decoded `title`, zoned `start`/`end` (e.g. `2026-11-03T16:00:00-06:00`),
`all_day`, `timezone`, `url`, `cost`, `venue` (name/address/city/region/postal_code/country),
`organizers`, `categories`, `tags`, `image`, `status`. `--format full` prints every field. `--format series` prints one record per series (see below).
Password-protected tec/v1 events are skipped in compact form.

## Date windows

`Bound` values: `today`, `week`, `month`, `year`, `all`, `YYYY-MM-DD`, or an RFC 3339 timestamp.
Windows line up with the calendar in the zone you choose; the default zone is the system one, and the
site's own zone is usually the better choice.

| keyword | as `start` | as `end` (exclusive) |
|---|---|---|
| `today` | today 00:00 | tomorrow 00:00 |
| `week` | first day of this week (Monday by default, configurable) | first day of next week |
| `month` | 1st of this month | 1st of next month |
| `year` | Jan 1 | Jan 1 next year |
| `all` | unbounded | unbounded |
| a date | that day 00:00 | the following day 00:00, so the date itself is included |

`RangeMode::StartsWithin` (the default) matches events that **start** inside the window.
`RangeMode::Overlaps` also matches events that began earlier and are still running.

How the window is sent to each API:
- **Legacy:** the client turns on `strict_dates` and sends exact zoned bounds. This also switches
  off the server's hidden default window (yesterday to +24 months), which otherwise hides past
  events.
- **tec/v1:** the bounds are sent directly; this API has no default window.

## Parallel fetching

`CrawlOptions { per_page, concurrency, max_pages, dedupe }`

- Page 1 is fetched first to learn the total page count. Pages 2..N are then fetched with up to
  `concurrency` requests at once.
- Results come out **in page order**.
- If a later page returns "page not found" (the data shrank mid-crawl), the stream ends cleanly.
- `dedupe` (on by default) drops items repeated across page boundaries when events are published
  during a crawl.

GET requests are retried on 429, 502, 503, 504 and on connection errors, with exponential backoff
that honours `Retry-After` (see `RetryPolicy`).

A live check against demo.theeventscalendar.com crawled 28 of 28 events with concurrency 6.

## TLS

The client uses rustls with the `ring` crypto backend; there is no OpenSSL dependency. Roots are
chosen at runtime with `TlsRoots`, and each root source is behind a cargo feature (both are on by
default):

| `TlsRoots` | Feature | Use when |
|---|---|---|
| `System` (default) | `system-roots` | You want the OS trust store: corporate CAs, OS updates (rustls-platform-verifier) |
| `Bundled` | `bundled-roots` | `FROM scratch` / distroless images with no CA bundle (Mozilla roots via webpki-roots) |
| `Custom(certs)` / `TlsRoots::custom_pem(..)` | always available | A private CA or pinned roots |

To ship only one source, depend with `default-features = false, features = ["bundled-roots"]`.
`ClientBuilder::http_client(reqwest::Client)` accepts a fully custom client instead.

## Authentication

Reading published content needs no authentication. For writes, drafts and private items, use
`Auth::ApplicationPassword` (WordPress 5.6+, over HTTPS), `Auth::Bearer` (JWT/OAuth plugins) or
`Auth::CookieNonce`.

## Series and recurrence

**Linking occurrences to series.** Both APIs encode series membership in the permalink:
`/event/{series-slug}/{date}/` for occurrences of an Events Calendar Pro recurring series, and
`/event/{slug}/` for one-off events.

- **Any event:** `event.permalink()?.series()` returns the series slug (`SeriesSlug`).
- **Legacy API, occurrence to series:** `events().series_parent(&slug)` returns the parent post
  (by-slug lookup).
- **Legacy API, series to occurrences:** `events().series_occurrences(parent_id, opts)` returns
  every occurrence, using `include=<parent>`.
- **tec/v1:** each row already carries the parent (`post.id`, `Event::series_parent()`) and the
  occurrence (`occurrence_id`).
- **`SeriesIndex::build(&events, &GroupOptions)`** groups any set of events. `series_of(id)` maps
  an occurrence id to its series.
- **Repeating one-off posts:** with `implied: true` (the default), these are grouped too, when they
  share the same title, venue, organizers and start time. Much of iondistrict's programming is
  entered this way: 105 such groups, versus 38 Pro series.

**Inferring the RRULE.** Neither API exposes the rule an editor configured.
`Series::infer_recurrence()` recovers the simplest rule that explains the observed dates:

- the frequencies tried are DAILY, WEEKLY (interval 1–4), MONTHLY by day of month, MONTHLY by
  nth or last weekday (`2TH`, `-1FR`), and YEARLY;
- `EXDATE`s cover skipped dates (holidays) and `RDATE`s cover extra ones;
- each result has a `Confidence` of `Exact`, `WithExceptions` or `Weak` (fewer than 4
  occurrences);
- `InferredRecurrence::to_ical()` writes the `DTSTART`/`DURATION`/`RRULE`/`EXDATE` lines.

On iondistrict.com the inference agrees exactly with an independent python-dateutil
implementation (`tools/infer_rrule.py`) for all 38 Pro series. This is enforced by a test against
the real data. Limits:

- `UNTIL` is the last occurrence seen, because Pro only generates occurrences a limited time ahead.
- Hidden events look like EXDATEs.
- With only 2 occurrences no rule is inferred. Houston Open Source Society, for example, needs its
  organizer's "third Wednesday" to be known.

```sh
cargo run -p tec-crawl -- --site https://iondistrict.com --start all --end all --tz America/Chicago --format series
cargo run -p tec-client --example series -- https://iondistrict.com 10004983
```

## Typing decisions

The goal is strong types that still decode real responses:

- **URLs** generated by the server (permalinks, REST URLs, images, terms, directions) are
  `url::Url`. Links typed in by editors (`website`, `virtual_url`) are `WebLink`, which keeps the
  text verbatim when it isn't a valid URL.
- **Rendered text** (titles, descriptions, excerpts, schedule details, cost, venue and organizer
  names) is `Html`. `.to_text()` strips tags and decodes entities.
- **Time zones** are `WpTimeZone`, and `.to_time_zone()` handles IANA names and `UTC+5.5`.
  Durations are `jiff::SignedDuration`, and PHP timezone kinds are `PhpTimezoneType`.
- **Ids** are newtypes (`EventId`, `VenueId`, `OrganizerId`, `TermId`, `UserId`, `AttachmentId`).
  They accept both `12` and `"12"`.
- **Dates** are `jiff` values.
  - `0000-00-00 00:00:00`, `false` and `""` become `None`.
  - Events have helpers: `start_instant()`, `start_zoned()`, and `time_zone()`, which handles
    WordPress `UTC+5.5` pseudo-zones.
- **Open sets** are enums with an `Other(String)` fallback: `PostStatus`, `Taxonomy`, `PostType`,
  `CurrencyPosition`, `EventOrderBy`, …
- **Polymorphic fields are normalised:**
  - legacy `venue` (`[]`, one object, or a list) becomes `venues: Vec<Venue>`;
  - `image: false` becomes `None`;
  - `custom_fields: []` becomes an empty map;
  - `ticketed` (bool or a provider list) becomes the `Ticketed` enum;
  - tec `multiday` (`false` or a day count) becomes `Option<u32>`;
  - tec `duration` (a numeric string) becomes `Option<u64>`.
- **Password-protected posts:**
  - tec/v1 entities are `Protectable<T>`: `Visible(T)`, or `PasswordProtected(stub)`. This is
    needed because the server rewrites every field of a protected post.
  - Legacy events report `is_password_blanked()`, and their boolean fields are `Option<bool>`.
- **Write bodies encode what is required:**
  - `NewEvent::new(title, start, end)` requires the mandatory fields; update structs make every
    field optional.
  - Inline venue and organizer creation use `VenueRef` and `OrganizerRef`.
- **Unknown response keys** are kept in an `extra` map rather than rejected.

## Server quirks the client handles

- Booleans are sent as `1`/`0`: tec/v1 casts `"false"` to **true**. `DELETE ?force=false` would
  permanently delete, so the client sends `force` in a JSON body instead.
- Lists are sent as `x[]=` (tec/v1) or as comma-separated values (legacy). Empty parameters are
  never sent; the legacy API returns 400 for them.
- tec/v1 requires the `X-TEC-EEA` acknowledgement header on every call. The client sends it.
- Pages past the end return a 404, and its code differs per endpoint. The paginator treats it as end
  of data.
- On password-protected organizers and venues, the server blanks the **name** field to `[]`. The
  client treats this as `None`.

## Server quirks you should know about (documented on the types)

- **Legacy updates are not partial.** Omitting a field resets `all_day`, `show_map`,
  `show_map_link`, `sticky`, `featured` and `hide_from_listings` to false, **deletes the featured
  image**, and **publishes drafts**. See `EventUpdate`.
- **tec/v1 updates also reset fields.** When omitted, `comment_status` and `ping_status` are set to
  `open` and `sticky` to `false`. Separately, `title`, `content` and `excerpt` have their HTML
  stripped on write.
- **Some tec/v1 event filters do nothing.** `categories` and `tags` are accepted but ignored in
  TEC ≤ 6.18.
- **Missing ids in tec/v1** return 401/403, not 404.
- **Hidden events** ("hide from listings") never appear in legacy `/events`.

## Tests

```sh
cargo test        # 40 tests
```

- **Unit tests:** date ranges, query encoding, request bodies, pagination order and dedupe.
- **Fixture tests:** every response snapshot from the plugin's own test suite (both APIs, including
  password-protected cases), plus one live response from the demo site, and the series and RRULE
  inference against all recurring series on iondistrict.com.
- **Mock-server tests (wiremock):** a multi-page crawl, page-not-found ending a crawl, typed
  errors, retries, the tec/v1 header and pagination headers, the DELETE body, and REST-root
  discovery for plain permalinks.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT)
at your option.

The test fixtures under `crates/tec-client/tests/fixtures/` include response snapshots from The
Events Calendar's test suite (GPL-2.0-or-later) and public event data captured from
demo.theeventscalendar.com and iondistrict.com. They are used for testing only and are not
included in the published crates.

## Suggested next steps

1. **A preserving `patch()` for legacy updates.** It would fetch the event, merge in your changes,
   and send the full state, avoiding the reset-on-omit behaviour above. Titles come back
   HTML-entity-encoded, so this needs an entity decoder (for example `html-escape`).
2. **Incremental sync.** Neither API filters by `modified`. Crawl the whole window and diff on
   `(id, modified_utc)`, or store `global_id` for cross-site identity.
3. **schema.org output.** Legacy single-event GETs include `json_ld` (an `Event` with `Place` and
   `Offers`). A `to_schema_org()` helper could build the same from archive items, which don't
   include it.
4. **Site time zone in `tec-crawl`.** The REST index exposes it publicly (`timezone_string`,
   falling back to `gmt_offset`), and `Client::site_info()` reads it; `tec-crawl` could default
   `--tz` from there instead of requiring it.
5. **Rate limiting.** Add a per-host token bucket if you crawl many sites concurrently.
