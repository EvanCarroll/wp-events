//! Query-string building that respects the servers' parsing quirks.
//!
//! * Booleans are always `1`/`0`: tec/v1 casts with PHP `(bool)`, so `"false"` would be true.
//! * Lists use `key[]=a&key[]=b`: tec/v1 rejects a single scalar without a comma.
//! * Empty strings are never sent: the legacy API answers 400 to `search=`.

use std::fmt::Display;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Encoded query parameters (opaque outside this crate).
pub struct Query(pub(crate) Vec<(String, String)>);

impl Query {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, key: &str, value: impl Display) -> &mut Self {
        let value = value.to_string();
        if !value.is_empty() {
            self.0.push((key.to_owned(), value));
        }
        self
    }

    pub(crate) fn opt(&mut self, key: &str, value: Option<impl Display>) -> &mut Self {
        if let Some(v) = value {
            self.push(key, v);
        }
        self
    }

    pub(crate) fn opt_bool(&mut self, key: &str, value: Option<bool>) -> &mut Self {
        if let Some(b) = value {
            self.push(key, if b { "1" } else { "0" });
        }
        self
    }

    pub(crate) fn list<I, T>(&mut self, key: &str, values: I) -> &mut Self
    where
        I: IntoIterator<Item = T>,
        T: Display,
    {
        let k = format!("{key}[]");
        for v in values {
            self.push(&k, v);
        }
        self
    }

    pub(crate) fn pairs(&self) -> &[(String, String)] {
        &self.0
    }
}
