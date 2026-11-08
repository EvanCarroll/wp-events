//! End-to-end behaviour against a mock WordPress.

use std::time::Duration;

use futures::TryStreamExt;
use serde_json::json;
use tec_client::jiff::tz::TimeZone;
use tec_client::tec::params::DeleteMode;
use tec_client::{Bound, Client, CrawlOptions, DateRange, EventId, RestRoot, RetryPolicy};
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

fn legacy_event(id: u64) -> serde_json::Value {
    json!({
        "id": id, "author": "1", "status": "publish",
        "rest_url": format!("https://ex.com/wp-json/tribe/events/v1/events/{id}"),
        "url": format!("https://ex.com/event/event-{id}/"),
        "title": format!("Event {id}"), "image": false, "venue": [], "organizer": [],
        "custom_fields": [], "start_date": "2026-11-12 10:00:00", "cost_details": {
            "currency_symbol": "$", "currency_code": "USD", "currency_position": "postfix", "values": ["10", 25]
        }
    })
}

fn client(server: &MockServer) -> Client {
    Client::builder(&server.uri())
        .unwrap()
        .retry(RetryPolicy {
            max_retries: 2,
            base_delay: Duration::from_millis(5),
            max_delay: Duration::from_millis(20),
        })
        .build()
        .unwrap()
}

/// Serves legacy archive pages of 2 events each, 3 pages total.
struct LegacyPages;
impl Respond for LegacyPages {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let page: u64 = req
            .url
            .query_pairs()
            .find(|(k, _)| k == "page")
            .map(|(_, v)| v.parse().unwrap())
            .unwrap_or(1);
        ResponseTemplate::new(200).set_body_json(json!({
            "events": [legacy_event(page * 10), legacy_event(page * 10 + 1)],
            "rest_url": "x", "total": 6, "total_pages": 3
        }))
    }
}

#[tokio::test]
async fn legacy_crawl_fetches_all_pages_with_window_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wp-json/tribe/events/v1/events"))
        .and(query_param("strict_dates", "true"))
        .and(query_param("per_page", "2"))
        .and(query_param_is_missing("end_date"))
        .respond_with(LegacyPages)
        .expect(3)
        .mount(&server)
        .await;

    let range = DateRange::new(Bound::Week, Bound::Month).time_zone(TimeZone::UTC);
    let opts = CrawlOptions {
        per_page: 2,
        concurrency: 2,
        ..Default::default()
    };
    let events: Vec<_> = client(&server)
        .tribe()
        .events()
        .crawl(&range, Default::default(), opts)
        .unwrap()
        .try_collect()
        .await
        .unwrap();
    let ids: Vec<u64> = events.iter().map(|e| e.id.get()).collect();
    assert_eq!(ids, vec![10, 11, 20, 21, 30, 31]);
    let cd = events[0].cost_details.as_ref().unwrap();
    assert_eq!(
        cd.values,
        vec![
            tec_client::tribe::models::CostValue::Amount("10".into()),
            tec_client::tribe::models::CostValue::Amount("25".into())
        ]
    );
    assert_eq!(cd.currency_position.as_str(), "suffix");

    let reqs = server.received_requests().await.unwrap();
    let q = reqs[0].url.query().unwrap().to_owned();
    assert!(
        q.contains("start_date=") && q.contains("starts_before="),
        "{q}"
    );
}

#[tokio::test]
async fn legacy_page_not_found_ends_crawl_cleanly() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wp-json/tribe/events/v1/events"))
        .and(query_param("page", "3"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "code": "event-archive-page-not-found",
            "message": "The requested event archive page does not exist",
            "data": {"status": 404}
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wp-json/tribe/events/v1/events"))
        .respond_with(LegacyPages)
        .mount(&server)
        .await;

    let events: Vec<_> = client(&server)
        .tribe()
        .events()
        .pages(Default::default(), CrawlOptions::default())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn legacy_api_errors_are_typed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wp-json/tribe/events/v1/events/99"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "code": "rest-event-not-found", "message": "Event with ID 99 does not exist.", "data": {"status": 404}
        })))
        .mount(&server)
        .await;
    let err = client(&server)
        .tribe()
        .events()
        .get(EventId(99))
        .await
        .unwrap_err();
    let tec_client::Error::Api(api) = err else {
        panic!("{err}")
    };
    assert_eq!(api.status.as_u16(), 404);
    assert_eq!(api.code(), Some("rest-event-not-found"));
}

#[tokio::test]
async fn retries_transient_failures() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wp-json/tribe/events/v1/events/5"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wp-json/tribe/events/v1/events/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(legacy_event(5)))
        .mount(&server)
        .await;
    let e = client(&server)
        .tribe()
        .events()
        .get(EventId(5))
        .await
        .unwrap();
    assert_eq!(e.id.get(), 5);
}

fn tec_event(id: u64) -> serde_json::Value {
    json!({
        "id": id, "date": "2025-06-13T17:25:00", "date_gmt": "2025-06-13T17:25:00",
        "guid": {"rendered": "g"}, "modified": "2025-06-13T17:25:00", "modified_gmt": "2025-06-13T17:25:00",
        "slug": "s", "status": "publish", "type": "tribe_events", "link": format!("https://ex.com/event/s-{id}/"),
        "title": {"rendered": "T"}, "content": {"rendered": "", "protected": false},
        "excerpt": {"rendered": "", "protected": false}, "author": 1, "featured_media": 0,
        "template": "", "tags": [], "tribe_events_cat": [], "class_list": [],
        "start_date": "2026-11-12 10:00:00", "duration": "3600", "multiday": false,
        "organizers": [], "venues": []
    })
}

#[tokio::test]
async fn tec_sends_ack_header_and_reads_pagination_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wp-json/tec/v1/events"))
        // `header()` would split the sentence on its comma; compare the raw value.
        .and(|req: &Request| {
            req.headers
                .get("x-tec-eea")
                .is_some_and(|v| v.as_bytes() == tec_client::tec::EXPERIMENTAL_ACK.as_bytes())
        })
        .and(query_param("orderby", "event_date"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-WP-Total", "3")
                .insert_header("X-WP-TotalPages", "2")
                .set_body_json(json!([tec_event(1), tec_event(2)])),
        )
        .mount(&server)
        .await;
    let q = tec_client::tec::params::EventQuery {
        orderby: Some(tec_client::tec::params::EventOrderBy::EventDate),
        ..Default::default()
    };
    let page = client(&server).tec().events().list(&q).await.unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.total_pages, 2);
    let e = page.items[0].visible().unwrap();
    assert_eq!(
        e.duration,
        Some(tec_client::jiff::SignedDuration::from_secs(3600))
    );
    assert_eq!(e.multiday, None);
}

#[tokio::test]
async fn tec_delete_sends_force_in_body() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/wp-json/tec/v1/venues/7"))
        .and(body_json(json!({"force": false})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    client(&server)
        .tec()
        .venues()
        .delete(tec_client::VenueId(7), DeleteMode::Trash)
        .await
        .unwrap();
}

#[tokio::test]
async fn discovers_plain_permalink_root() {
    let server = MockServer::start().await;
    let root = format!("{}/?rest_route=/", server.uri());
    Mock::given(method("HEAD"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Link", format!("<{root}>; rel=\"https://api.w.org/\"")),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/"))
        .and(query_param("rest_route", "/tribe/events/v1/events/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(legacy_event(5)))
        .mount(&server)
        .await;
    let c = Client::builder(&server.uri())
        .unwrap()
        .discover()
        .await
        .unwrap();
    assert!(matches!(c.rest_root(), RestRoot::QueryParam(_)));
    assert_eq!(
        c.tribe().events().get(EventId(5)).await.unwrap().id.get(),
        5
    );
}

fn tec_occurrence(id: u64, start: &str) -> serde_json::Value {
    let mut e = tec_event(id);
    e["start_date"] = json!(start);
    e["start_date_utc"] = json!(start);
    e["class_list"] = json!(["tribe-recurring-event", "tribe-recurring-event-parent"]);
    e["link"] = json!(format!("https://ex.com/event/meetup/{}/", &start[..10]));
    e["occurrence_id"] =
        json!(10_000_000 + start[..10].replace('-', "").parse::<u64>().unwrap() % 1_000_000);
    e
}

/// tec/v1 returns every occurrence of a series under the parent's id, including occurrences
/// outside the requested window. The crawler must keep distinct occurrences and drop the
/// out-of-window ones.
#[tokio::test]
async fn tec_recurring_occurrences_share_parent_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wp-json/tec/v1/events"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-WP-Total", "3")
                .insert_header("X-WP-TotalPages", "1")
                .set_body_json(json!([
                    tec_occurrence(60334, "2026-07-09 22:30:00"),
                    tec_occurrence(60334, "2026-08-13 22:30:00"),
                    tec_occurrence(60334, "2027-01-14 23:30:00"),
                ])),
        )
        .mount(&server)
        .await;
    let c = client(&server);

    let all: Vec<_> = c
        .tec()
        .events()
        .crawl_all(Default::default(), CrawlOptions::default())
        .try_collect()
        .await
        .unwrap();
    assert_eq!(all.len(), 3, "occurrences must not be de-duplicated away");
    assert!(all[0].visible().unwrap().is_recurring());

    let july = DateRange::new(
        Bound::Date("2026-07-01".parse().unwrap()),
        Bound::Date("2026-07-31".parse().unwrap()),
    )
    .time_zone(TimeZone::UTC);
    let in_july: Vec<_> = c
        .tec()
        .events()
        .crawl(&july, Default::default(), CrawlOptions::default())
        .unwrap()
        .try_collect()
        .await
        .unwrap();
    assert_eq!(in_july.len(), 1);
    assert_eq!(
        in_july[0]
            .visible()
            .unwrap()
            .start_date
            .unwrap()
            .to_string(),
        "2026-07-09T22:30:00"
    );
}

#[tokio::test]
async fn oversized_bodies_are_refused() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wp-json/tribe/events/v1/events/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(legacy_event(7)))
        .mount(&server)
        .await;
    let capped = Client::builder(&server.uri())
        .unwrap()
        .max_body_bytes(64)
        .build()
        .unwrap();
    let err = capped.tribe().events().get(EventId(7)).await.unwrap_err();
    assert!(
        matches!(err, tec_client::Error::BodyTooLarge { limit: 64, .. }),
        "{err:?}"
    );
    // The same response under a roomy cap decodes.
    let roomy = Client::builder(&server.uri())
        .unwrap()
        .max_body_bytes(1 << 20)
        .build()
        .unwrap();
    assert_eq!(
        roomy.tribe().events().get(EventId(7)).await.unwrap().id,
        EventId(7)
    );
}

#[tokio::test]
async fn site_info_asks_for_only_its_fields() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wp-json/"))
        .and(query_param(
            "_fields",
            "name,description,url,home,timezone_string,gmt_offset",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "Ion District", "url": "https://iondistrict.com",
            "home": "https://iondistrict.com", "gmt_offset": -6,
            "timezone_string": "America/Chicago"
        })))
        .mount(&server)
        .await;
    let info = client(&server).site_info().await.unwrap();
    assert_eq!(info.name.to_text(), "Ion District");
    assert_eq!(
        info.time_zone().unwrap(),
        TimeZone::get("America/Chicago").unwrap()
    );
}
