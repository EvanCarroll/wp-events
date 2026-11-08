//! The site itself, from the WordPress REST index (`/wp-json/`).
//!
//! The index is public on every site whose REST API is, and it is where a site's own zone is
//! readable without authentication. It is also large (hundreds of KB of route listings), so
//! [`Client::site_info`] asks for just these fields with `_fields`.

use jiff::tz::TimeZone;
use reqwest::Method;
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

use crate::client::Client;
use crate::error::Result;
use crate::query::Query;
use crate::types::{Html, WpTimeZone};
use crate::wire;

/// The fields of the REST index that describe the site.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SiteInfo {
    /// Site title; entity-encoded.
    #[serde(default)]
    pub name: Html,
    /// Tagline; entity-encoded.
    #[serde(default)]
    pub description: Html,
    /// WordPress address.
    #[serde(default, deserialize_with = "wire::url_opt")]
    pub url: Option<Url>,
    /// Site (front-end) address.
    #[serde(default, deserialize_with = "wire::url_opt")]
    pub home: Option<Url>,
    /// IANA zone name; empty when the site is set to a bare UTC offset.
    #[serde(default)]
    pub timezone_string: String,
    /// Hours from UTC (may be fractional, e.g. `5.5`). Some hosts send it as a string.
    #[serde(default, deserialize_with = "offset_hours")]
    pub gmt_offset: Option<f64>,
}

impl SiteInfo {
    /// The site's zone: the named zone when set, else a fixed zone from `gmt_offset`.
    pub fn time_zone(&self) -> Option<TimeZone> {
        WpTimeZone(self.timezone_string.clone())
            .to_time_zone()
            .or_else(|| {
                let hours = self.gmt_offset?;
                WpTimeZone(format!("UTC{hours:+}")).to_time_zone()
            })
    }
}

fn offset_hours<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<f64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Number(f64),
        Text(String),
    }
    Ok(match Option::<Raw>::deserialize(d)? {
        Some(Raw::Number(n)) => Some(n),
        Some(Raw::Text(s)) => s.trim().parse().ok(),
        None => None,
    })
}

impl Client {
    /// The site's name, addresses and zone, from the REST index.
    pub async fn site_info(&self) -> Result<SiteInfo> {
        let mut query = Query::new();
        query.push(
            "_fields",
            "name,description,url,home,timezone_string,gmt_offset",
        );
        self.send(Method::GET, &[], &query, None, &[]).await?.json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(json: serde_json::Value) -> SiteInfo {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn named_zone_wins() {
        let i = info(serde_json::json!({
            "name": "Ion District", "url": "https://iondistrict.com",
            "home": "https://iondistrict.com", "gmt_offset": -6,
            "timezone_string": "America/Chicago"
        }));
        assert_eq!(i.time_zone().unwrap().iana_name(), Some("America/Chicago"));
        assert_eq!(i.name.to_text(), "Ion District");
    }

    #[test]
    fn offset_only_sites_get_a_fixed_zone() {
        let i = info(serde_json::json!({"timezone_string": "", "gmt_offset": "5.5"}));
        let tz = i.time_zone().unwrap();
        assert_eq!(
            tz.to_offset(jiff::Timestamp::UNIX_EPOCH).seconds(),
            5 * 3600 + 1800
        );
    }

    #[test]
    fn nothing_means_none() {
        assert!(info(serde_json::json!({})).time_zone().is_none());
    }
}
