//! Link event occurrences to the series they belong to.
//!
//! Two kinds of series are recognised:
//!
//! * **Recurring** (Events Calendar Pro): occurrences share the permalink
//!   `/event/{series-slug}/{date}/`. tec/v1 also reports the parent post id directly; with the
//!   legacy API resolve it with [`crate::tribe::Events::series_parent`].
//! * **Implied**: one-off posts that repeat with the same title, venue, organizers and start
//!   time (e.g. a meetup an editor re-creates every month). Opt-in via
//!   [`GroupOptions::implied`].
//!
//! [`SeriesIndex::build`] groups any mix of legacy and tec/v1 events, and
//! [`Series::infer_recurrence`] recovers an RRULE.

use std::collections::{BTreeMap, HashMap};

use jiff::Zoned;
use jiff::tz::TimeZone;
use serde::Serialize;

use crate::permalink::SeriesSlug;
use crate::recurrence::{self, InferOptions, InferredRecurrence, Observed};
use crate::tec::models as tec;
use crate::tribe::models as tribe;
use crate::types::{EventId, OrganizerId, VenueId};

/// What makes events one series.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", content = "key", rename_all = "snake_case")]
pub enum SeriesKey {
    /// An Events Calendar Pro recurring series, by the parent's slug.
    Recurring(SeriesSlug),
    /// Repeating one-off posts: normalised title, venue, organizers, local start time.
    Implied(String),
}

/// One occurrence in a series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Occurrence {
    /// Unique per occurrence.
    pub id: EventId,
    #[serde(serialize_with = "ser_zoned")]
    pub start: Zoned,
    #[serde(serialize_with = "ser_zoned_opt")]
    pub end: Option<Zoned>,
}

fn ser_zoned<S: serde::Serializer>(z: &Zoned, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(&z.strftime("%Y-%m-%dT%H:%M:%S%:z"))
}

fn ser_zoned_opt<S: serde::Serializer>(z: &Option<Zoned>, s: S) -> Result<S::Ok, S::Error> {
    match z {
        Some(z) => ser_zoned(z, s),
        None => s.serialize_none(),
    }
}

/// A group of occurrences.
#[derive(Debug, Clone, Serialize)]
pub struct Series {
    pub key: SeriesKey,
    /// The parent post (tec/v1 rows carry it; for legacy rows use
    /// [`crate::tribe::Events::series_parent`]).
    pub parent: Option<EventId>,
    /// Display title (decoded).
    pub title: String,
    /// Sorted by start.
    pub occurrences: Vec<Occurrence>,
}

impl Series {
    pub fn infer_recurrence(&self, opts: InferOptions) -> Option<InferredRecurrence> {
        let obs: Vec<Observed> = self
            .occurrences
            .iter()
            .map(|o| Observed {
                start: o.start.clone(),
                end: o.end.clone(),
            })
            .collect();
        recurrence::infer(&obs, opts)
    }
}

/// Anything that can be placed in a series.
pub trait SeriesMember {
    /// `None` when the event has no usable start time.
    fn occurrence(&self, site_tz: &TimeZone) -> Option<Occurrence>;
    fn series_slug(&self) -> Option<SeriesSlug>;
    fn series_parent(&self) -> Option<EventId>;
    fn title_text(&self) -> String;
    fn venue(&self) -> Option<VenueId>;
    fn organizers(&self) -> Vec<OrganizerId>;
}

impl SeriesMember for tribe::Event {
    fn occurrence(&self, site_tz: &TimeZone) -> Option<Occurrence> {
        Some(Occurrence {
            id: self.id,
            start: self.start_zoned(site_tz)?,
            end: self.end_zoned(site_tz),
        })
    }
    fn series_slug(&self) -> Option<SeriesSlug> {
        self.permalink()?.series()
    }
    fn series_parent(&self) -> Option<EventId> {
        None
    }
    fn title_text(&self) -> String {
        self.title.to_text()
    }
    fn venue(&self) -> Option<VenueId> {
        self.venues.first().map(|v| v.id)
    }
    fn organizers(&self) -> Vec<OrganizerId> {
        self.organizers.iter().map(|o| o.id).collect()
    }
}

impl SeriesMember for tec::Event {
    fn occurrence(&self, _site_tz: &TimeZone) -> Option<Occurrence> {
        Some(Occurrence {
            id: self.occurrence_id.unwrap_or(self.post.id),
            start: self.start_zoned()?,
            end: self.end_zoned(),
        })
    }
    fn series_slug(&self) -> Option<SeriesSlug> {
        self.permalink()?.series()
    }
    fn series_parent(&self) -> Option<EventId> {
        tec::Event::series_parent(self)
    }
    fn title_text(&self) -> String {
        self.post.title.rendered.to_text()
    }
    fn venue(&self) -> Option<VenueId> {
        self.venues.first().map(tec::Protectable::id)
    }
    fn organizers(&self) -> Vec<OrganizerId> {
        self.organizers.iter().map(tec::Protectable::id).collect()
    }
}

/// Options for [`SeriesIndex::build`].
#[derive(Debug, Clone)]
pub struct GroupOptions {
    /// Zone for legacy events without one (normally the site's zone).
    pub site_tz: TimeZone,
    /// Also group repeating one-off posts into [`SeriesKey::Implied`] series.
    pub implied: bool,
    /// Minimum one-off posts to form an implied series. Default 2.
    pub min_implied: usize,
}

impl Default for GroupOptions {
    fn default() -> Self {
        Self {
            site_tz: TimeZone::system(),
            implied: true,
            min_implied: 2,
        }
    }
}

/// Series found in a set of events, with a lookup from occurrence id to series.
#[derive(Debug, Clone, Default)]
pub struct SeriesIndex {
    series: Vec<Series>,
    by_event: HashMap<EventId, usize>,
}

impl SeriesIndex {
    pub fn build<'a, E, I>(events: I, opts: &GroupOptions) -> Self
    where
        E: SeriesMember + 'a,
        I: IntoIterator<Item = &'a E>,
    {
        let mut groups: BTreeMap<SeriesKey, Series> = BTreeMap::new();
        let mut add = |key: SeriesKey, parent: Option<EventId>, title: String, occ: Occurrence| {
            let s = groups.entry(key.clone()).or_insert_with(|| Series {
                key,
                parent: None,
                title,
                occurrences: Vec::new(),
            });
            s.parent = s.parent.or(parent);
            if !s.occurrences.iter().any(|o| o.id == occ.id) {
                s.occurrences.push(occ);
            }
        };
        let mut singles: Vec<(String, String, Occurrence)> = Vec::new();
        for e in events {
            let Some(occ) = e.occurrence(&opts.site_tz) else {
                continue;
            };
            match e.series_slug() {
                Some(slug) => add(
                    SeriesKey::Recurring(slug),
                    e.series_parent(),
                    e.title_text(),
                    occ,
                ),
                None if opts.implied => {
                    let mut orgs: Vec<u64> = e.organizers().iter().map(|o| o.get()).collect();
                    orgs.sort_unstable();
                    let key = format!(
                        "{}|venue:{}|org:{:?}|{}|{}",
                        normalise(&e.title_text()),
                        e.venue().map_or(0, VenueId::get),
                        orgs,
                        occ.start.strftime("%H:%M"),
                        occ.start.time_zone().iana_name().unwrap_or("?"),
                    );
                    singles.push((key, e.title_text(), occ));
                }
                None => {}
            }
        }
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for (k, _, _) in &singles {
            *counts.entry(k.as_str()).or_default() += 1;
        }
        let keep: Vec<String> = counts
            .into_iter()
            .filter(|&(_, n)| n >= opts.min_implied.max(2))
            .map(|(k, _)| k.to_owned())
            .collect();
        for (key, title, occ) in singles {
            if keep.contains(&key) {
                add(SeriesKey::Implied(key), None, title, occ);
            }
        }
        let mut series: Vec<Series> = groups.into_values().collect();
        let mut by_event = HashMap::new();
        for (i, s) in series.iter_mut().enumerate() {
            s.occurrences.sort_by(|a, b| a.start.cmp(&b.start));
            for o in &s.occurrences {
                by_event.insert(o.id, i);
            }
        }
        Self { series, by_event }
    }

    pub fn series(&self) -> &[Series] {
        &self.series
    }

    /// The series an occurrence (by its occurrence id) belongs to.
    pub fn series_of(&self, id: EventId) -> Option<&Series> {
        self.by_event.get(&id).map(|&i| &self.series[i])
    }
}

fn normalise(title: &str) -> String {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
