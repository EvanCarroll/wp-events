//! Link an occurrence to its series and recover the series' rule.
//!
//! ```sh
//! cargo run -p tec-client --example series -- https://iondistrict.com 10004983
//! ```

use tec_client::jiff::tz::TimeZone;
use tec_client::{Client, CrawlOptions, EventId, InferOptions, Series, SeriesKey, SeriesMember};

#[tokio::main]
async fn main() -> tec_client::Result<()> {
    let mut args = std::env::args().skip(1);
    let site = args.next().expect("site URL");
    let id: EventId = args
        .next()
        .expect("occurrence id")
        .parse()
        .expect("numeric id");
    let tz = TimeZone::get("America/Chicago")?;

    let events = Client::builder(&site)?.discover().await?.tribe().events();
    let occurrence = events.get(id).await?;
    println!(
        "occurrence {}: {} at {:?}",
        occurrence.id,
        occurrence.title.to_text(),
        occurrence.start_zoned(&tz)
    );

    let Some(slug) = occurrence.series_slug() else {
        println!("not part of a recurring series");
        return Ok(());
    };
    let parent = events.series_parent(&slug).await?;
    println!("series {slug}: parent post {}", parent.id);

    let all = events
        .series_occurrences(parent.id, CrawlOptions::default())
        .await?;
    let series = Series {
        key: SeriesKey::Recurring(slug),
        parent: Some(parent.id),
        title: parent.title.to_text(),
        occurrences: all.iter().filter_map(|e| e.occurrence(&tz)).collect(),
    };
    println!("{} occurrences", series.occurrences.len());
    match series.infer_recurrence(InferOptions::default()) {
        Some(r) => print!("confidence: {:?}\n{}", r.confidence, r.to_ical()),
        None => println!("no rule fits"),
    }
    Ok(())
}
