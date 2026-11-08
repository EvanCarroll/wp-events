//! Decode every response snapshot shipped with The Events Calendar's own test suite
//! (placeholders such as `{EVENT_ID}` replaced with 4242).

use std::path::PathBuf;

use tec_client::tec::models as tec;
use tec_client::tec::models::Protectable;
use tec_client::tribe::models as tribe;

fn fixtures(dir: &str) -> Vec<(String, serde_json::Value)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir);
    let mut out: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let v = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
            (name, v)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!out.is_empty());
    out
}

fn decode<T: serde::de::DeserializeOwned>(name: &str, v: serde_json::Value) -> T {
    serde_json::from_value(v).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn tribe_snapshots_decode() {
    for (name, v) in fixtures("tribe") {
        if let Some(events) = v.get("events") {
            let events: Vec<tribe::Event> = decode(&name, events.clone());
            assert!(!events.is_empty(), "{name}");
            for e in &events {
                assert_eq!(e.id.get(), 4242);
                assert!(e.start_date.is_some(), "{name}: start_date");
            }
        } else if let Some(items) = v.get("venues") {
            let _: Vec<tribe::Venue> = decode(&name, items.clone());
        } else if let Some(items) = v.get("organizers") {
            let _: Vec<tribe::Organizer> = decode(&name, items.clone());
        } else if v.get("venue").is_some() {
            let _: tribe::Venue = decode(&name, v);
        } else {
            let _: tribe::Organizer = decode(&name, v);
        }
    }
}

#[test]
fn tribe_custom_fields_and_password_blanking() {
    let all = fixtures("tribe");
    let (_, v) = all
        .iter()
        .find(|(n, _)| n.contains("show_additional_fields"))
        .unwrap();
    let events: Vec<tribe::Event> = serde_json::from_value(v["events"].clone()).unwrap();
    let with_fields = events.iter().find(|e| !e.custom_fields.is_empty()).unwrap();
    assert_eq!(
        with_fields.custom_fields["_ecp_custom_1"].label,
        "Cool Field"
    );

    let (_, v) = all
        .iter()
        .find(|(n, _)| n.starts_with("Archive_EventTest__it_should_hide_password"))
        .unwrap();
    let events: Vec<tribe::Event> = serde_json::from_value(v["events"].clone()).unwrap();
    assert!(events.iter().any(tribe::Event::is_password_blanked));
    assert!(events.iter().any(|e| !e.is_password_blanked()));
}

#[test]
fn tec_snapshots_decode() {
    for (name, v) in fixtures("tec") {
        if name.starts_with("Event") {
            let items: Vec<Protectable<tec::Event>> = decode(&name, v);
            for item in &items {
                if let Protectable::Visible(e) = item {
                    assert!(e.dates.is_some(), "{name}: dates");
                    assert!(e.start_zoned().is_some(), "{name}: start_zoned");
                    assert!(e.duration.is_some(), "{name}: duration");
                }
            }
        } else if name.starts_with("Venue") {
            let _: Vec<Protectable<tec::Venue>> = decode(&name, v);
        } else {
            let _: Vec<Protectable<tec::Organizer>> = decode(&name, v);
        }
    }
}

#[test]
fn tec_password_protected_is_detected() {
    let mut protected = 0;
    let mut visible = 0;
    for (name, v) in fixtures("tec") {
        if !name.starts_with("Event") {
            continue;
        }
        let items: Vec<Protectable<tec::Event>> = serde_json::from_value(v).unwrap();
        for i in items {
            match i {
                Protectable::Visible(e) => {
                    visible += 1;
                    assert!(!e.class_list.iter().any(|c| c == "post-password-required"));
                }
                Protectable::PasswordProtected(p) => {
                    protected += 1;
                    assert_eq!(p.id, tec_client::EventId(4242));
                    assert!(!p.title.rendered.is_empty());
                }
            }
        }
    }
    assert!(
        protected > 0 && visible > 0,
        "protected={protected} visible={visible}"
    );
}

#[test]
fn tec_multiday_and_directions() {
    let all = fixtures("tec");
    let multi = all
        .iter()
        .filter(|(n, _)| n.starts_with("Events_"))
        .flat_map(|(_, v)| {
            serde_json::from_value::<Vec<Protectable<tec::Event>>>(v.clone()).unwrap()
        })
        .filter_map(Protectable::into_visible)
        .filter_map(|e| e.multiday)
        .max();
    assert_eq!(multi, Some(4));

    let (_, v) = all.iter().find(|(n, _)| n.starts_with("Venues_")).unwrap();
    let venues: Vec<Protectable<tec::Venue>> = serde_json::from_value(v.clone()).unwrap();
    let link = venues
        .iter()
        .filter_map(Protectable::visible)
        .find_map(|v| v.directions_link.clone())
        .unwrap();
    assert!(!link.as_str().contains("&#038;"), "{link}");
}

/// A real response from demo.theeventscalendar.com (TEC + Event Tickets + Pro), 2026-11-08.
#[test]
fn live_demo_site_decodes() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/live_demo_events.json");
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let events: Vec<tribe::Event> = serde_json::from_value(v["events"].clone()).unwrap();
    assert_eq!(events.len(), 5);
    let first = &events[0];
    assert_eq!(
        first.ticketed,
        Some(tec_client::Ticketed::Providers(vec!["woo".into()]))
    );
    assert_eq!(first.venues.len(), 1);
    let tz = first.time_zone(&tec_client::jiff::tz::TimeZone::UTC);
    assert_eq!(tz.iana_name(), Some("America/Detroit"));
    assert_eq!(
        first
            .start_zoned(&tec_client::jiff::tz::TimeZone::UTC)
            .unwrap()
            .to_string(),
        "2026-11-09T18:30:00-05:00[America/Detroit]"
    );
}

/// Series grouping and RRULE inference on real data: every recurring series on iondistrict.com
/// (312 occurrences, 38 series) plus the Houston Open Source Society's one-off posts.
/// Expected rules were produced independently by `tools/infer_rrule.py` (python-dateutil).
#[test]
fn ion_series_inference_matches_reference() {
    use std::collections::BTreeMap;
    use tec_client::{GroupOptions, InferOptions, SeriesIndex, SeriesKey};

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("ion_series_events.json")).unwrap())
            .unwrap();
    let events: Vec<tribe::Event> = serde_json::from_value(v["events"].clone()).unwrap();
    let expected: BTreeMap<String, Option<serde_json::Value>> =
        serde_json::from_slice(&std::fs::read(dir.join("ion_series_expected.json")).unwrap())
            .unwrap();

    let opts = GroupOptions {
        site_tz: tec_client::jiff::tz::TimeZone::get("America/Chicago").unwrap(),
        ..Default::default()
    };
    let index = SeriesIndex::build(&events, &opts);
    let recurring: Vec<_> = index
        .series()
        .iter()
        .filter(|s| matches!(s.key, SeriesKey::Recurring(_)))
        .collect();
    assert_eq!(recurring.len(), 38);
    assert_eq!(
        recurring.iter().map(|s| s.occurrences.len()).sum::<usize>(),
        312
    );

    for s in &recurring {
        let SeriesKey::Recurring(slug) = &s.key else {
            unreachable!()
        };
        let got = s.infer_recurrence(InferOptions::default()).map(|r| {
            serde_json::json!({
                "rule": r.rule.to_string().split(";UNTIL").next().unwrap(),
                "exdates": r.exdates.iter().map(|z| z.date().to_string()).collect::<Vec<_>>(),
                "rdates": r.rdates.iter().map(|z| z.date().to_string()).collect::<Vec<_>>(),
            })
        });
        assert_eq!(&got, &expected[&slug.0], "series {slug}");
        // Every occurrence links back to its series.
        for o in &s.occurrences {
            assert_eq!(index.series_of(o.id).map(|x| &x.key), Some(&s.key));
        }
    }

    // HOSS: two one-off posts, same title/venue/organizer/time → one implied series, too short
    // to infer a rule from.
    let hoss: Vec<_> = index
        .series()
        .iter()
        .filter(|s| s.title == "Houston Open Source Society Meetup")
        .collect();
    assert_eq!(hoss.len(), 1);
    assert!(matches!(hoss[0].key, SeriesKey::Implied(_)));
    assert_eq!(hoss[0].occurrences.len(), 2);
    assert!(hoss[0].infer_recurrence(InferOptions::default()).is_none());
}
