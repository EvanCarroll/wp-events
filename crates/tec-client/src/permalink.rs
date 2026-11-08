//! Event permalinks, which encode series membership.
//!
//! Events Calendar Pro gives each occurrence of a recurring series the URL
//! `/event/{series-slug}/{YYYY-MM-DD}/`, where `{series-slug}` is the parent post's slug. One-off
//! events are `/event/{slug}/`. With plain permalinks the same information is in
//! `?tribe_events={slug}&eventDate={date}`.

use jiff::civil::Date;
use serde::Serialize;
use url::Url;

/// The series slug of a recurring series (the parent post's slug). Resolve it to the parent
/// post with [`crate::tribe::Events::get_by_slug`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SeriesSlug(pub String);

impl std::fmt::Display for SeriesSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A parsed event permalink.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct EventPermalink {
    /// The post slug: for an occurrence, the series (parent) slug.
    pub slug: String,
    /// The occurrence date for occurrences of a recurring series; `None` for one-off events.
    pub occurrence_date: Option<Date>,
}

impl EventPermalink {
    pub fn parse(url: &Url) -> Option<Self> {
        let q = |k: &str| {
            url.query_pairs()
                .find(|(name, _)| name == k)
                .map(|(_, v)| v.into_owned())
        };
        if let Some(slug) = q("tribe_events").filter(|s| !s.is_empty()) {
            let occurrence_date = q("eventDate").and_then(|d| d.parse().ok());
            return Some(Self {
                slug,
                occurrence_date,
            });
        }
        let segs: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
        match segs.as_slice() {
            [.., slug, last] if is_date(last) => Some(Self {
                slug: (*slug).to_owned(),
                occurrence_date: last.parse().ok(),
            }),
            [_, .., slug] => Some(Self {
                slug: (*slug).to_owned(),
                occurrence_date: None,
            }),
            _ => None,
        }
    }

    /// `Some` when this permalink is an occurrence of a recurring series.
    pub fn series(&self) -> Option<SeriesSlug> {
        self.occurrence_date.map(|_| SeriesSlug(self.slug.clone()))
    }
}

fn is_date(s: &str) -> bool {
    s.len() == 10 && s.parse::<Date>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Option<EventPermalink> {
        EventPermalink::parse(&Url::parse(s).unwrap())
    }

    #[test]
    fn pretty_and_plain() {
        let o = p("https://iondistrict.com/event/upstream-digital-tech-meetup-at-second-draught-3/2026-11-12/").unwrap();
        assert_eq!(o.slug, "upstream-digital-tech-meetup-at-second-draught-3");
        assert_eq!(o.occurrence_date.unwrap().to_string(), "2026-11-12");
        assert_eq!(o.series().unwrap().0, o.slug);

        let one = p("https://iondistrict.com/event/houston-open-source-society-meetup-2/").unwrap();
        assert_eq!(one.slug, "houston-open-source-society-meetup-2");
        assert!(one.series().is_none());

        let plain =
            p("http://wordpress.test/?tribe_events=test-event-1&eventDate=2024-07-14").unwrap();
        assert_eq!(plain.slug, "test-event-1");
        assert!(plain.series().is_some());
        assert!(
            p("http://wordpress.test/?tribe_events=test-event-1")
                .unwrap()
                .series()
                .is_none()
        );
    }
}
