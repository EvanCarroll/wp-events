//! How requests reach the server.
//!
//! [`crate::Client`] decides *what* to ask — the URL, method, authentication and body — and what
//! an answer means; a [`Transport`] decides *how* the bytes travel. Every request the client
//! makes goes through the one transport it was built with: REST-root discovery, the site index,
//! each page of a crawl, each write.
//!
//! [`ReqwestTransport`] (the `reqwest` feature, on by default) is the built-in one. A caller
//! with an HTTP policy of its own — a pinned resolver, an allowlist, a per-host concurrency
//! cap, a body cap it must enforce itself — implements the trait over its own client and hands
//! it to [`crate::ClientBuilder::transport`]; the client then never holds an HTTP client of its
//! own.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use http::{HeaderMap, Method, StatusCode};
use url::Url;

/// An error a transport reports, kept whole so a caller that knows the transport can read it
/// back (`downcast`).
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// One request as the client composed it. Authentication, `Content-Type` and any `User-Agent`
/// the client was given are already in `headers`.
#[derive(Debug, Clone)]
pub struct TransportRequest {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
    /// The client's whole-request deadline, body included. A transport with a stricter bound
    /// of its own keeps that one.
    pub timeout: Duration,
    /// Refuse a body past this many bytes with [`TransportError::BodyTooLarge`], without
    /// reading it whole. `None` is unlimited.
    pub max_body_bytes: Option<u64>,
}

/// A complete answer, body read. Every status comes back here, not only 2xx: the client reads
/// the API's error bodies to classify them.
#[derive(Debug, Clone)]
pub struct TransportResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    /// Where the body came from, after any redirects the transport followed.
    pub url: Url,
    pub body: Vec<u8>,
}

/// Why a transport produced no response.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The server could not be reached, or did not answer in time. The client retries these
    /// for idempotent requests under its [`crate::RetryPolicy`].
    #[error("unreachable: {0}")]
    Unreachable(#[source] BoxError),
    /// The transport's own policy declined to send the request — an address or host it will not
    /// contact, a redirect it will not follow. Never retried: asking again changes nothing.
    #[error("refused: {0}")]
    Refused(#[source] BoxError),
    /// The body passed [`TransportRequest::max_body_bytes`].
    #[error("response from {url} exceeds the {limit}-byte body limit")]
    BodyTooLarge { url: String, limit: u64 },
    /// Anything else.
    #[error("{0}")]
    Other(#[source] BoxError),
}

impl TransportError {
    /// The transport's own error, for the caller that lent the transport and knows its type.
    pub fn into_source(self) -> Option<BoxError> {
        match self {
            Self::Unreachable(e) | Self::Refused(e) | Self::Other(e) => Some(e),
            Self::BodyTooLarge { .. } => None,
        }
    }
}

/// A way to send one HTTP request and read the whole answer.
pub trait Transport: Send + Sync + std::fmt::Debug + 'static {
    fn send<'a>(
        &'a self,
        req: TransportRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, TransportError>> + Send + 'a>>;
}

/// The built-in transport: a `reqwest::Client`.
#[cfg(feature = "reqwest")]
#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

#[cfg(feature = "reqwest")]
impl ReqwestTransport {
    /// Over a client configured by the caller (proxies, client certificates, a resolver).
    /// [`TransportRequest::timeout`] and [`TransportRequest::max_body_bytes`] still bound
    /// every request.
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[cfg(feature = "reqwest")]
impl Transport for ReqwestTransport {
    fn send<'a>(
        &'a self,
        req: TransportRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, TransportError>> + Send + 'a>> {
        Box::pin(async move {
            let mut builder = self
                .client
                .request(req.method, req.url)
                .timeout(req.timeout)
                .headers(req.headers);
            if let Some(body) = req.body {
                builder = builder.body(body);
            }
            let resp = builder.send().await.map_err(classify)?;
            let status = resp.status();
            let headers = resp.headers().clone();
            let url = resp.url().clone();
            let body = read_body(resp, req.max_body_bytes, &url).await?;
            Ok(TransportResponse {
                status,
                headers,
                url,
                body,
            })
        })
    }
}

/// Timeouts and connection failures are the network's; everything else is this request's.
#[cfg(feature = "reqwest")]
fn classify(e: reqwest::Error) -> TransportError {
    if e.is_timeout() || e.is_connect() || e.is_request() {
        TransportError::Unreachable(Box::new(e))
    } else {
        TransportError::Other(Box::new(e))
    }
}

/// The whole body, refused once it passes `limit` — the chunk that would cross it is never
/// appended and the rest never pulled off the socket.
#[cfg(feature = "reqwest")]
async fn read_body(
    mut resp: reqwest::Response,
    limit: Option<u64>,
    url: &Url,
) -> Result<Vec<u8>, TransportError> {
    let Some(limit) = limit else {
        return resp
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| TransportError::Other(Box::new(e)));
    };
    let too_large = || TransportError::BodyTooLarge {
        url: url.to_string(),
        limit,
    };
    if resp.content_length().is_some_and(|len| len > limit) {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| TransportError::Other(Box::new(e)))?
    {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
