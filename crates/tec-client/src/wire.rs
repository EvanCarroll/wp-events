//! Serde helpers for the many irregular wire encodings these APIs use.
//!
//! PHP serialises loosely: ids arrive as `"12"` or `12`, "no value" is `false`, `""`, `[]`
//! or `"0000-00-00 00:00:00"`, and an empty map is `[]`. These helpers convert all of that
//! into plain Rust types so the public models never expose the mess.

use std::collections::BTreeMap;
use std::fmt;

use jiff::civil;
use serde::Deserialize;
use serde::de::{self, DeserializeOwned, Deserializer, Visitor};

/// Define a string-backed enum with an `Other(String)` catch-all so new server-side values
/// never break decoding. Generates `as_str`, `Display`, `FromStr`, and serde impls.
#[macro_export]
#[doc(hidden)]
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident = $wire:literal $(| $alias:literal)* ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        $vis enum $name {
            $( $(#[$vmeta])* $variant, )+
            /// A value this library does not know about.
            Other(String),
        }

        impl $name {
            /// The wire representation.
            pub fn as_str(&self) -> &str {
                match self {
                    $( $name::$variant => $wire, )+
                    $name::Other(s) => s.as_str(),
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = ::std::convert::Infallible;
            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                Ok(match s {
                    $( $wire $(| $alias)* => $name::$variant, )+
                    other => $name::Other(other.to_owned()),
                })
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, s: S) -> ::std::result::Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> ::std::result::Result<Self, D::Error> {
                let s = <::std::borrow::Cow<'de, str> as ::serde::Deserialize>::deserialize(d)?;
                Ok(s.parse().unwrap_or_else(|e: ::std::convert::Infallible| match e {}))
            }
        }
    };
}

/// A JSON scalar that may be a number or a numeric string.
#[derive(Deserialize)]
#[serde(untagged)]
enum NumLike {
    U(u64),
    I(i64),
    F(f64),
    S(String),
    B(bool),
}

impl NumLike {
    fn into_u64<E: de::Error>(self) -> Result<u64, E> {
        match self {
            NumLike::U(n) => Ok(n),
            NumLike::I(n) if n >= 0 => Ok(n as u64),
            NumLike::F(f) if f >= 0.0 && f.fract() == 0.0 => Ok(f as u64),
            NumLike::S(s) => s
                .trim()
                .parse()
                .map_err(|_| E::custom(format!("expected an unsigned integer, got {s:?}"))),
            NumLike::I(n) => Err(E::custom(format!("expected an unsigned integer, got {n}"))),
            NumLike::F(f) => Err(E::custom(format!("expected an unsigned integer, got {f}"))),
            NumLike::B(b) => Err(E::custom(format!("expected an unsigned integer, got {b}"))),
        }
    }
}

/// `12` or `"12"` → `u64`.
pub(crate) fn u64_lenient<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    NumLike::deserialize(d)?.into_u64()
}

/// `0`, `"0"`, `""`, `false`, `null` → `None`; any other number → `Some`.
pub(crate) fn u64_zero_none<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    match Option::<NumLike>::deserialize(d)? {
        None | Some(NumLike::B(false)) => Ok(None),
        Some(NumLike::S(s)) if s.trim().is_empty() => Ok(None),
        Some(n) => n.into_u64().map(|n| (n != 0).then_some(n)),
    }
}

/// Like [`u64_zero_none`] but into an id newtype.
pub(crate) fn id_zero_none<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: From<u64>,
{
    u64_zero_none(d).map(|o| o.map(T::from))
}

/// A non-empty string → `Some`; `""`, `null`, `false`, `[]` (password blanking) or absent → `None`.
pub(crate) fn empty_none<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

/// Like [`empty_none`] but wraps the string (e.g. into [`crate::types::Html`]).
pub(crate) fn empty_none_as<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: From<String>,
{
    empty_none(d).map(|o| o.map(T::from))
}

/// Server-generated URL: `""`/`false`/`null` → `None`; otherwise it must parse.
pub(crate) fn url_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<url::Url>, D::Error> {
    match empty_none(d)? {
        None => Ok(None),
        Some(s) => url::Url::parse(s.trim())
            .map(Some)
            .map_err(|e| de::Error::custom(format!("invalid URL {s:?}: {e}"))),
    }
}

/// Editor-entered link: kept as text when it is not a valid URL.
pub(crate) fn weblink_opt<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<crate::types::WebLink>, D::Error> {
    Ok(empty_none(d)?.and_then(|s| crate::types::WebLink::parse(&s)))
}

/// `false` → `None`, anything else decodes as `T`. Also accepts `null`/absence.
pub(crate) fn false_or<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Bool(false) | serde_json::Value::Null => Ok(None),
        other => serde_json::from_value(other)
            .map(Some)
            .map_err(de::Error::custom),
    }
}

/// `[]`, `{...}`, or `[{...}, ...]` → `Vec<T>`.
pub(crate) fn one_or_many<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(items) => items
            .into_iter()
            .map(|i| serde_json::from_value(i).map_err(de::Error::custom))
            .collect(),
        // A PHP array with non-sequential keys serialises as an object of items.
        serde_json::Value::Object(ref m)
            if m.keys().all(|k| k.parse::<u64>().is_ok()) && !m.is_empty() =>
        {
            let serde_json::Value::Object(m) = v else {
                unreachable!()
            };
            m.into_iter()
                .map(|(_, i)| serde_json::from_value(i).map_err(de::Error::custom))
                .collect()
        }
        other => serde_json::from_value(other)
            .map(|one| vec![one])
            .map_err(de::Error::custom),
    }
}

/// `[]` (PHP empty array) or `{...}` → map.
pub(crate) fn empty_array_or_map<'de, D, T>(d: D) -> Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Null => Ok(BTreeMap::new()),
        serde_json::Value::Array(a) if a.is_empty() => Ok(BTreeMap::new()),
        other => serde_json::from_value(other).map_err(de::Error::custom),
    }
}

/// PHP `DateTime` strings: `Y-m-d H:i:s`, `Y-m-d\TH:i:s`, optional fraction and offset
/// (an offset is dropped: callers know which zone the field is in).
pub(crate) fn parse_civil(s: &str) -> Option<civil::DateTime> {
    let s = s.trim();
    if s.is_empty() || s.starts_with("0000-00-00") {
        return None;
    }
    if let Ok(dt) = s.parse::<civil::DateTime>() {
        return Some(dt);
    }
    // Strip a trailing `Z` / `±HH:MM` and retry.
    let cut = s
        .strip_suffix('Z')
        .or_else(|| s.rfind(['+', '-']).filter(|&i| i > 10).map(|i| &s[..i]))?;
    cut.parse::<civil::DateTime>().ok()
}

/// Required civil date-time.
pub(crate) fn civil_datetime<'de, D: Deserializer<'de>>(d: D) -> Result<civil::DateTime, D::Error> {
    let s = String::deserialize(d)?;
    parse_civil(&s).ok_or_else(|| de::Error::custom(format!("invalid date-time {s:?}")))
}

/// Optional civil date-time: `null`, `""`, `false` and the MySQL zero date become `None`.
pub(crate) fn civil_datetime_opt<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<civil::DateTime>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum In {
        S(String),
        B(#[allow(dead_code)] bool),
    }
    match Option::<In>::deserialize(d)? {
        None | Some(In::B(_)) => Ok(None),
        Some(In::S(s)) => {
            if s.trim().is_empty() || s.starts_with("0000-00-00") {
                return Ok(None);
            }
            parse_civil(&s)
                .map(Some)
                .ok_or_else(|| de::Error::custom(format!("invalid date-time {s:?}")))
        }
    }
}

/// `["2024-01-01", ...]` → `Vec<civil::Date>`.
pub(crate) fn civil_dates<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<civil::Date>, D::Error> {
    let raw = Vec::<String>::deserialize(d)?;
    raw.iter()
        .map(|s| s.parse::<civil::Date>().map_err(de::Error::custom))
        .collect()
}

/// A zero-padded numeric string (`"07"`) or number → integer type.
pub(crate) fn padded<'de, D, N>(d: D) -> Result<N, D::Error>
where
    D: Deserializer<'de>,
    N: TryFrom<u64>,
{
    let n = NumLike::deserialize(d)?.into_u64::<D::Error>()?;
    N::try_from(n).map_err(|_| de::Error::custom(format!("{n} out of range")))
}

/// `false` → `None`, a whole number → `Some(n)`. Used by tec/v1 `multiday`.
pub(crate) fn false_or_u32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    match Option::<NumLike>::deserialize(d)? {
        None | Some(NumLike::B(false)) => Ok(None),
        Some(NumLike::B(true)) => Ok(Some(1)),
        Some(n) => {
            let n = n.into_u64::<D::Error>()?;
            u32::try_from(n).map(Some).map_err(de::Error::custom)
        }
    }
}

/// A number, numeric string, or slug string, kept as text. Used by legacy `cost_details.values`.
pub(crate) fn string_or_number<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    struct V;
    impl Visitor<'_> for V {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a string or number")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_owned())
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<String, E> {
            Ok(v.to_string())
        }
    }
    d.deserialize_any(V)
}

/// Deserialise a `Vec<String>` whose items may be numbers.
pub(crate) fn strings_or_numbers<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    struct W(#[serde(deserialize_with = "string_or_number")] String);
    let v = Vec::<W>::deserialize(d)?;
    Ok(v.into_iter().map(|w| w.0).collect())
}

/// Accept `[]`, `{...}`, or a list and produce a `Vec<T>` (for lists PHP may emit as objects).
pub(crate) fn list_lenient<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    one_or_many(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_variants() {
        assert!(parse_civil("2024-05-01 18:00:00").is_some());
        assert!(parse_civil("2024-05-01T18:00:00").is_some());
        assert!(parse_civil("2023-01-15 09:00:00.000000").is_some());
        assert!(parse_civil("2024-05-01T18:00:00+02:00").is_some());
        assert!(parse_civil("2024-05-01T18:00:00Z").is_some());
        assert!(parse_civil("0000-00-00 00:00:00").is_none());
        assert!(parse_civil("Password protected").is_none());
    }
}
