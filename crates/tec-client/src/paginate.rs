//! Parallel pagination shared by both APIs.
//!
//! Page 1 is fetched first to learn `total_pages`; pages `2..=N` are then fetched with up to
//! `concurrency` requests in flight. Results are yielded **in page order** (`buffered`, not
//! `buffer_unordered`), so output is deterministic. A "page not found" 404 on a later page (the
//! data shrank mid-crawl) ends the stream cleanly instead of failing it.

use std::collections::HashSet;
use std::future::{Future, ready};
use std::hash::Hash;

use futures::stream::{self, BoxStream, StreamExt, TryStreamExt};
use serde::Serialize;

use crate::error::{Error, Result};

/// One page of results.
#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// 1-based page number.
    pub page: u32,
    /// Total matching items across all pages.
    pub total: u64,
    /// Total pages at the effective page size (0 when there are no results).
    pub total_pages: u32,
}

impl<T> Page<T> {
    pub fn has_next(&self) -> bool {
        self.page < self.total_pages
    }
}

/// Options for crawling every page of a collection.
#[derive(Debug, Clone, Copy)]
pub struct CrawlOptions {
    /// Page size. The legacy API silently caps this at 50 (site-filterable); tec/v1 accepts more.
    pub per_page: u32,
    /// Maximum requests in flight. Be polite: most WordPress hosts are small.
    pub concurrency: usize,
    /// Stop after this many pages.
    pub max_pages: Option<u32>,
    /// Drop items already seen (by id). Guards against duplicates when the data shifts between
    /// page requests (an event is published mid-crawl and pushes others to the next page).
    pub dedupe: bool,
}

impl Default for CrawlOptions {
    fn default() -> Self {
        Self {
            per_page: 50,
            concurrency: 4,
            max_pages: None,
            dedupe: true,
        }
    }
}

/// Stream all pages: page 1 sequentially, then the rest concurrently, in order.
pub(crate) fn pages<T, F, Fut>(opts: CrawlOptions, fetch: F) -> BoxStream<'static, Result<Page<T>>>
where
    T: Send + 'static,
    F: Fn(u32) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<Page<T>>> + Send + 'static,
{
    let concurrency = opts.concurrency.max(1);
    let max_pages = opts.max_pages;
    let first = fetch(1);
    stream::once(first)
        .map(move |res| -> BoxStream<'static, Result<Page<T>>> {
            match res {
                Err(e) => stream::once(ready(Err(e))).boxed(),
                Ok(p) => {
                    let last = match max_pages {
                        Some(m) => p.total_pages.min(m),
                        None => p.total_pages,
                    };
                    let fetch = fetch.clone();
                    let rest = stream::iter(2..=last)
                        .map(fetch)
                        .buffered(concurrency)
                        .take_while(|r| {
                            ready(!matches!(r, Err(Error::Api(e)) if e.is_page_not_found()))
                        });
                    stream::once(ready(Ok(p))).chain(rest).boxed()
                }
            }
        })
        .flatten()
        .boxed()
}

/// Flatten pages into items, optionally de-duplicating by key.
pub(crate) fn items<T, K, F>(
    pages: BoxStream<'static, Result<Page<T>>>,
    dedupe: bool,
    key: F,
) -> BoxStream<'static, Result<T>>
where
    T: Send + 'static,
    K: Eq + Hash + Send + 'static,
    F: Fn(&T) -> K + Send + 'static,
{
    let flat = pages
        .map_ok(|p| stream::iter(p.items.into_iter().map(Ok)))
        .try_flatten();
    if !dedupe {
        return flat.boxed();
    }
    let mut seen = HashSet::new();
    flat.filter(move |r| {
        let keep = match r {
            Ok(item) => seen.insert(key(item)),
            Err(_) => true,
        };
        ready(keep)
    })
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn fetches_all_pages_in_order() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let fetch = move |n: u32| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                // Later pages finish first to prove ordering is preserved.
                tokio::time::sleep(std::time::Duration::from_millis(u64::from(10 - n))).await;
                Ok(Page {
                    items: vec![n * 10, n * 10 + 1],
                    page: n,
                    total: 10,
                    total_pages: 5,
                })
            }
        };
        let opts = CrawlOptions {
            concurrency: 3,
            ..Default::default()
        };
        let all: Vec<u32> = items(pages(opts, fetch), true, |x| *x)
            .try_collect()
            .await
            .unwrap();
        assert_eq!(all, vec![10, 11, 20, 21, 30, 31, 40, 41, 50, 51]);
        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn dedupes_and_respects_max_pages() {
        let fetch = |n: u32| async move {
            Ok(Page {
                items: vec![1u32, n],
                page: n,
                total: 100,
                total_pages: 50,
            })
        };
        let opts = CrawlOptions {
            max_pages: Some(3),
            ..Default::default()
        };
        let all: Vec<u32> = items(pages(opts, fetch), true, |x| *x)
            .try_collect()
            .await
            .unwrap();
        assert_eq!(all, vec![1, 2, 3]);
    }
}
