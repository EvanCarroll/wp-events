//! The HTTP client shared by both API modules.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, RETRY_AFTER};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::{ApiError, Error, ErrorBody, Result};
use crate::query::Query;
use crate::tls::TlsRoots;

const DEFAULT_USER_AGENT: &str = concat!("tec-client/", env!("CARGO_PKG_VERSION"));

/// How to authenticate. Reads of published content need none.
#[derive(Clone, Default)]
#[non_exhaustive]
pub enum Auth {
    #[default]
    None,
    /// WordPress Application Passwords (WP 5.6+) over HTTP Basic. Requires HTTPS by default.
    ApplicationPassword { username: String, password: String },
    /// A bearer token from a JWT/OAuth plugin.
    Bearer(String),
    /// A logged-in browser session: the raw `Cookie` header and a `wp_rest` nonce.
    CookieNonce { cookie: String, nonce: String },
}

impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Auth::None => f.write_str("None"),
            Auth::ApplicationPassword { username, .. } => f
                .debug_struct("ApplicationPassword")
                .field("username", username)
                .field("password", &"<redacted>")
                .finish(),
            Auth::Bearer(_) => f.write_str("Bearer(<redacted>)"),
            Auth::CookieNonce { .. } => f.write_str("CookieNonce(<redacted>)"),
        }
    }
}

/// Retry policy for idempotent requests (GET) on 429, 502, 503, 504 and transport errors.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Attempts after the first. `0` disables retries.
    pub max_retries: u32,
    /// Delay before the first retry; doubles each time. `Retry-After` wins when present.
    pub base_delay: Duration,
    /// Upper bound for any single delay.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
        }
    }
}

/// Where the WordPress REST API lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestRoot {
    /// Pretty permalinks: `https://site/wp-json/` (routes are appended as path segments).
    Path(Url),
    /// Plain permalinks: `https://site/?rest_route=` (routes go in the `rest_route` parameter).
    QueryParam(Url),
}

impl RestRoot {
    /// Interpret a root URL as returned in the `Link: <...>; rel="https://api.w.org/"` header.
    pub fn from_url(mut url: Url) -> Self {
        if url.query_pairs().any(|(k, _)| k == "rest_route") {
            url.set_query(None);
            RestRoot::QueryParam(url)
        } else {
            if !url.path().ends_with('/') {
                let p = format!("{}/", url.path());
                url.set_path(&p);
            }
            RestRoot::Path(url)
        }
    }

    /// The default for a site URL: `{site}/wp-json/`.
    pub fn default_for_site(site: &Url) -> Result<Self> {
        let mut base = site.clone();
        if !base.path().ends_with('/') {
            let p = format!("{}/", base.path());
            base.set_path(&p);
        }
        Ok(RestRoot::Path(base.join("wp-json/")?))
    }

    /// Build the full URL for a route made of path segments (each is percent-encoded).
    pub(crate) fn url(&self, segments: &[&str], query: &Query) -> Result<Url> {
        let mut url = match self {
            RestRoot::Path(base) => {
                let mut u = base.clone();
                // No segments is the root itself, whose trailing slash is part of its address.
                if !segments.is_empty() {
                    let mut segs = u.path_segments_mut().map_err(|()| {
                        Error::Config(format!("REST root {base} cannot be a base URL"))
                    })?;
                    segs.pop_if_empty();
                    segs.extend(segments);
                }
                u
            }
            RestRoot::QueryParam(base) => {
                let mut u = base.clone();
                u.query_pairs_mut()
                    .append_pair("rest_route", &format!("/{}", segments.join("/")));
                u
            }
        };
        if !query.pairs().is_empty() {
            url.query_pairs_mut().extend_pairs(query.pairs());
        }
        Ok(url)
    }
}

/// Builder for [`Client`].
#[derive(Debug)]
pub struct ClientBuilder {
    site: Url,
    root: Option<RestRoot>,
    tls: TlsRoots,
    http: Option<reqwest::Client>,
    auth: Auth,
    user_agent: String,
    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    retry: RetryPolicy,
    max_body_bytes: Option<u64>,
}

/// Whole-request timeout when none is configured.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

impl ClientBuilder {
    /// Override the REST root instead of assuming `{site}/wp-json/`.
    pub fn rest_root(mut self, root: RestRoot) -> Self {
        self.root = Some(root);
        self
    }

    /// Choose which certificate authorities to trust.
    pub fn tls_roots(mut self, roots: TlsRoots) -> Self {
        self.tls = roots;
        self
    }

    /// Use a fully configured `reqwest::Client` (proxies, client certs, a hardened resolver,
    /// ...). TLS and connect-timeout options on this builder are then ignored; [`Self::timeout`]
    /// and [`Self::max_body_bytes`] still bound every request.
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.http = Some(client);
        self
    }

    pub fn auth(mut self, auth: Auth) -> Self {
        self.auth = auth;
        self
    }

    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }

    /// Whole-request timeout, body included. Default 60s.
    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = Some(t);
        self
    }

    /// Refuse a response body larger than `n` bytes with [`Error::BodyTooLarge`]. Unlimited by
    /// default. Counted as the body streams, so an oversized response is never fully read.
    pub fn max_body_bytes(mut self, n: u64) -> Self {
        self.max_body_bytes = Some(n);
        self
    }

    pub fn connect_timeout(mut self, t: Duration) -> Self {
        self.connect_timeout = Some(t);
        self
    }

    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = policy;
        self
    }

    fn http(&self) -> Result<reqwest::Client> {
        if let Some(c) = &self.http {
            return Ok(c.clone());
        }
        let mut b = reqwest::Client::builder()
            .tls_backend_preconfigured(self.tls.client_config()?)
            .user_agent(&self.user_agent)
            .timeout(self.timeout.unwrap_or(DEFAULT_TIMEOUT))
            .connect_timeout(self.connect_timeout.unwrap_or(Duration::from_secs(15)));
        if self.site.scheme() == "https" {
            b = b.https_only(true);
        }
        Ok(b.build()?)
    }

    /// Build the client using the configured (or default `/wp-json/`) REST root.
    pub fn build(self) -> Result<Client> {
        let http = self.http()?;
        let root = match self.root {
            Some(r) => r,
            None => RestRoot::default_for_site(&self.site)?,
        };
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                root,
                site: self.site,
                auth: self.auth,
                retry: self.retry,
                timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
                max_body_bytes: self.max_body_bytes,
            }),
        })
    }

    /// Build the client, discovering the REST root from the site's
    /// `Link: <…>; rel="https://api.w.org/"` header. Falls back to `/wp-json/`.
    pub async fn discover(mut self) -> Result<Client> {
        if self.root.is_none() {
            let http = self.http()?;
            let found = discover_root(&http, &self.site).await;
            self.http = Some(http);
            self.root = Some(match found {
                Some(r) => r,
                None => RestRoot::default_for_site(&self.site)?,
            });
        }
        self.build()
    }
}

async fn discover_root(http: &reqwest::Client, site: &Url) -> Option<RestRoot> {
    let resp = http.head(site.clone()).send().await.ok()?;
    let resp = if resp.headers().get(reqwest::header::LINK).is_none() {
        http.get(site.clone()).send().await.ok()?
    } else {
        resp
    };
    resp.headers()
        .get_all(reqwest::header::LINK)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(parse_link_header)
        .find(|(_, rel)| rel == "https://api.w.org/")
        .and_then(|(href, _)| Url::parse(&href).ok())
        .map(RestRoot::from_url)
}

/// Parse an RFC 8288 `Link` header into `(url, rel)` pairs.
pub(crate) fn parse_link_header(value: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    // URLs may contain commas; split on '<' boundaries instead.
    for part in value.split('<').skip(1) {
        let Some((href, params)) = part.split_once('>') else {
            continue;
        };
        let rel = params
            .split(';')
            .filter_map(|p| p.trim().strip_prefix("rel="))
            .map(|r| r.trim().trim_end_matches(',').trim_matches('"').to_owned())
            .next();
        if let Some(rel) = rel {
            out.push((href.trim().to_owned(), rel));
        }
    }
    out
}

/// A cheaply clonable handle to one WordPress site.
#[derive(Debug, Clone)]
pub struct Client {
    pub(crate) inner: Arc<Inner>,
}

#[derive(Debug)]
pub(crate) struct Inner {
    pub(crate) http: reqwest::Client,
    pub(crate) root: RestRoot,
    pub(crate) site: Url,
    pub(crate) auth: Auth,
    pub(crate) retry: RetryPolicy,
    pub(crate) timeout: Duration,
    pub(crate) max_body_bytes: Option<u64>,
}

/// A successful response with its headers kept (pagination lives there for tec/v1).
#[derive(Debug)]
pub(crate) struct RawResponse {
    pub(crate) headers: HeaderMap,
    pub(crate) url: String,
    pub(crate) body: Vec<u8>,
}

impl RawResponse {
    pub(crate) fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body).map_err(|source| Error::Decode {
            url: self.url.clone(),
            source,
        })
    }

    pub(crate) fn header_u64(&self, name: &str) -> Option<u64> {
        self.headers.get(name)?.to_str().ok()?.trim().parse().ok()
    }
}

impl Client {
    /// Start configuring a client for the site at `site` (its home URL).
    pub fn builder(site: &str) -> Result<ClientBuilder> {
        let site = Url::parse(site)?;
        Ok(ClientBuilder {
            site,
            root: None,
            tls: TlsRoots::default(),
            http: None,
            auth: Auth::None,
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            timeout: None,
            connect_timeout: None,
            retry: RetryPolicy::default(),
            max_body_bytes: None,
        })
    }

    /// Shorthand for `Client::builder(site)?.build()`.
    pub fn new(site: &str) -> Result<Self> {
        Self::builder(site)?.build()
    }

    /// The legacy `tribe/events/v1` API (works on virtually every TEC site).
    pub fn tribe(&self) -> crate::tribe::TribeApi {
        crate::tribe::TribeApi::new(self.clone())
    }

    /// The newer, experimental `tec/v1` API (TEC 6.15+).
    pub fn tec(&self) -> crate::tec::TecApi {
        crate::tec::TecApi::new(self.clone())
    }

    pub fn site(&self) -> &Url {
        &self.inner.site
    }

    pub fn rest_root(&self) -> &RestRoot {
        &self.inner.root
    }

    pub(crate) async fn send(
        &self,
        method: Method,
        segments: &[&str],
        query: &Query,
        body: Option<&serde_json::Value>,
        headers: &[(HeaderName, HeaderValue)],
    ) -> Result<RawResponse> {
        let url = self.inner.root.url(segments, query)?;
        let retry = self.inner.retry;
        let idempotent = method == Method::GET || method == Method::HEAD;
        let mut attempt = 0u32;
        loop {
            let mut req = self
                .inner
                .http
                .request(method.clone(), url.clone())
                .timeout(self.inner.timeout);
            req = match &self.inner.auth {
                Auth::None => req,
                Auth::ApplicationPassword { username, password } => {
                    req.basic_auth(username, Some(password))
                }
                Auth::Bearer(t) => req.bearer_auth(t),
                Auth::CookieNonce { cookie, nonce } => req
                    .header(reqwest::header::COOKIE, cookie)
                    .header("X-WP-Nonce", nonce),
            };
            for (k, v) in headers {
                req = req.header(k.clone(), v.clone());
            }
            if let Some(b) = body {
                req = req.json(b);
            }
            tracing::debug!(%method, %url, attempt, "request");
            let result = req.send().await;
            let retryable_after = match &result {
                Ok(r) if idempotent && is_retryable(r.status()) => Some(retry_after(r.headers())),
                Err(e) if idempotent && (e.is_timeout() || e.is_connect() || e.is_request()) => {
                    Some(None)
                }
                _ => None,
            };
            if let Some(hint) = retryable_after
                && attempt < retry.max_retries
            {
                let backoff = retry
                    .base_delay
                    .saturating_mul(2u32.saturating_pow(attempt));
                let delay = hint.unwrap_or(backoff).min(retry.max_delay);
                tracing::warn!(%url, attempt, ?delay, "retrying");
                tokio::time::sleep(delay).await;
                attempt += 1;
                continue;
            }
            let resp = result?;
            let status = resp.status();
            let headers = resp.headers().clone();
            let final_url = resp.url().to_string();
            let bytes = read_body(resp, self.inner.max_body_bytes, &final_url).await?;
            if !status.is_success() {
                let body = serde_json::from_slice::<ErrorBody>(&bytes).unwrap_or_else(|_| {
                    ErrorBody::Unparsed(String::from_utf8_lossy(&bytes).into_owned())
                });
                return Err(Error::Api(Box::new(ApiError {
                    status,
                    url: final_url,
                    body,
                })));
            }
            return Ok(RawResponse {
                headers,
                url: final_url,
                body: bytes,
            });
        }
    }
}

/// The whole body, refused once it passes `limit`.
async fn read_body(mut resp: reqwest::Response, limit: Option<u64>, url: &str) -> Result<Vec<u8>> {
    let Some(limit) = limit else {
        return Ok(resp.bytes().await?.to_vec());
    };
    let too_large = || Error::BodyTooLarge {
        url: url.to_owned(),
        limit,
    };
    if resp.content_length().is_some_and(|len| len > limit) {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn is_retryable(s: StatusCode) -> bool {
    matches!(
        s,
        StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

fn retry_after(h: &HeaderMap) -> Option<Duration> {
    h.get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_header() {
        let v = r#"<https://ex.com/wp-json/tec/v1/events?page=3&a=1,2>; rel="next", <https://ex.com/wp-json/tec/v1/events?page=1>; rel="prev""#;
        let l = parse_link_header(v);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].1, "next");
        assert!(l[0].0.ends_with("a=1,2"));
        assert_eq!(l[1].1, "prev");
    }

    #[test]
    fn root_urls() {
        let q = {
            let mut q = Query::new();
            q.push("page", 2);
            q
        };
        let p = RestRoot::default_for_site(&Url::parse("https://ex.com/blog").unwrap()).unwrap();
        assert_eq!(
            p.url(&["tribe", "events", "v1", "events", "by-slug", "a b"], &q)
                .unwrap()
                .as_str(),
            "https://ex.com/blog/wp-json/tribe/events/v1/events/by-slug/a%20b?page=2"
        );
        let r = RestRoot::from_url(Url::parse("https://ex.com/?rest_route=/").unwrap());
        assert_eq!(
            r.url(&["tec", "v1", "events"], &q).unwrap().as_str(),
            "https://ex.com/?rest_route=%2Ftec%2Fv1%2Fevents&page=2"
        );
    }
}
