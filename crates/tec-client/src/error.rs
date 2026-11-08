//! Error types for the client.

use reqwest::StatusCode;
use serde::Deserialize;

/// Everything that can go wrong while talking to a calendar.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Transport-level failure (DNS, TCP, TLS handshake, timeout, ...).
    #[error("HTTP transport error: {0}")]
    Http(#[from] reqwest::Error),

    /// A URL could not be built or parsed.
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),

    /// The TLS configuration could not be built.
    #[error("TLS configuration error: {0}")]
    Tls(#[from] rustls::Error),

    /// A header value we were asked to send is not valid.
    #[error("invalid header value: {0}")]
    InvalidHeader(#[from] reqwest::header::InvalidHeaderValue),

    /// The server answered with a non-success status code.
    #[error("{0}")]
    Api(#[from] Box<ApiError>),

    /// The response body did not match the expected shape.
    #[error("could not decode response from {url}: {source}")]
    Decode {
        url: String,
        #[source]
        source: serde_json::Error,
    },

    /// The response body passed [`crate::ClientBuilder::max_body_bytes`].
    #[error("response from {url} exceeds the {limit}-byte body limit")]
    BodyTooLarge { url: String, limit: u64 },

    /// A date range could not be resolved (e.g. a non-existent local time).
    #[error("date range error: {0}")]
    Time(#[from] jiff::Error),

    /// A builder or request was configured inconsistently.
    #[error("configuration error: {0}")]
    Config(String),

    /// No TLS root option is available with the enabled cargo features.
    #[error("TLS roots {0:?} requested but the corresponding cargo feature is not enabled")]
    TlsRootsUnavailable(&'static str),
}

/// A non-2xx response from the API.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{status} from {url}: {body}")]
pub struct ApiError {
    pub status: StatusCode,
    pub url: String,
    pub body: ErrorBody,
}

impl ApiError {
    /// The machine-readable error code, when the server sent one.
    pub fn code(&self) -> Option<&str> {
        match &self.body {
            ErrorBody::Wp(e) => Some(e.code.as_str()),
            ErrorBody::Simple { .. } | ErrorBody::Unparsed(_) => None,
        }
    }

    /// `true` for the "page past the end" 404s both APIs emit.
    pub fn is_page_not_found(&self) -> bool {
        self.status == StatusCode::NOT_FOUND
            && self.code().is_some_and(|c| {
                c.ends_with("archive-page-not-found") || c.ends_with("_page_not_found")
            })
    }

    /// `true` when the tribe/events/v1 API is disabled or absent on the site.
    pub fn is_no_route(&self) -> bool {
        self.code() == Some("rest_no_route")
    }
}

/// The three body shapes the APIs use for errors.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ErrorBody {
    /// Standard WordPress `WP_Error` JSON (`{code, message, data}`).
    Wp(WpError),
    /// tec/v1 handler errors: `{"error": "..."}` with no code.
    Simple { error: String },
    /// Anything else (HTML error pages from proxies, empty bodies, ...).
    #[serde(skip)]
    Unparsed(String),
}

impl std::fmt::Display for ErrorBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorBody::Wp(e) => write!(f, "{} ({})", e.message, e.code),
            ErrorBody::Simple { error } => f.write_str(error),
            ErrorBody::Unparsed(s) => {
                let s: String = s.chars().take(200).collect();
                write!(f, "unparsed body: {s}")
            }
        }
    }
}

/// WordPress `WP_Error` serialisation.
#[derive(Debug, Clone, Deserialize)]
pub struct WpError {
    pub code: String,
    pub message: String,
    /// Usually `{status, params?, details?}`; may be absent, null or a scalar.
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub additional_errors: Vec<WpError>,
}

impl WpError {
    /// Per-parameter messages from a `rest_invalid_param` error.
    pub fn invalid_params(&self) -> Vec<(String, String)> {
        self.data
            .as_ref()
            .and_then(|d| d.get("params"))
            .and_then(|p| p.as_object())
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
