//! A caller-supplied transport: what the client hands it, and how its failures come back.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

use http::{HeaderMap, StatusCode};
use serde_json::json;
use tec_client::{
    Auth, Client, Error, RetryPolicy, Transport, TransportError, TransportRequest,
    TransportResponse,
};
use url::Url;

/// Records what it is asked and answers from a script, one entry per request.
#[derive(Debug)]
struct Scripted {
    seen: Mutex<Vec<TransportRequest>>,
    answers: Mutex<Vec<Result<TransportResponse, TransportError>>>,
}

impl Scripted {
    fn new(answers: Vec<Result<TransportResponse, TransportError>>) -> Self {
        Self {
            seen: Mutex::new(Vec::new()),
            answers: Mutex::new(answers.into_iter().rev().collect()),
        }
    }
}

impl Transport for Scripted {
    fn send<'a>(
        &'a self,
        req: TransportRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, TransportError>> + Send + 'a>> {
        self.seen.lock().unwrap().push(req);
        let answer = self
            .answers
            .lock()
            .unwrap()
            .pop()
            .expect("the script covers every request");
        Box::pin(async move { answer })
    }
}

fn ok(body: serde_json::Value) -> Result<TransportResponse, TransportError> {
    Ok(TransportResponse {
        status: StatusCode::OK,
        headers: HeaderMap::new(),
        url: Url::parse("https://ex.com/wp-json/").unwrap(),
        body: serde_json::to_vec(&body).unwrap(),
    })
}

fn boxed(msg: &str) -> Box<dyn std::error::Error + Send + Sync> {
    msg.to_owned().into()
}

fn client(transport: Scripted, retries: u32) -> (Client, std::sync::Arc<Scripted>) {
    let transport = std::sync::Arc::new(transport);
    // `Arc<T: Transport>` is not itself a transport; hand the client a handle that forwards.
    #[derive(Debug)]
    struct Shared(std::sync::Arc<Scripted>);
    impl Transport for Shared {
        fn send<'a>(
            &'a self,
            req: TransportRequest,
        ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, TransportError>> + Send + 'a>>
        {
            self.0.send(req)
        }
    }
    let client = Client::builder("https://ex.com/")
        .unwrap()
        .transport(Shared(transport.clone()))
        .auth(Auth::ApplicationPassword {
            username: "ada".into(),
            password: "s3cret".into(),
        })
        .user_agent("probe/1")
        .timeout(Duration::from_secs(7))
        .max_body_bytes(4096)
        .retry(RetryPolicy {
            max_retries: retries,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
        })
        .build()
        .unwrap();
    (client, transport)
}

#[tokio::test]
async fn the_client_composes_the_request_and_the_transport_carries_it() {
    let (c, t) = client(
        Scripted::new(vec![ok(json!({"name": "Ex", "timezone_string": "UTC"}))]),
        0,
    );
    let info = c.site_info().await.unwrap();
    assert_eq!(info.name.to_text(), "Ex");

    let seen = t.seen.lock().unwrap();
    let req = &seen[0];
    assert_eq!(req.method, http::Method::GET);
    assert!(req.url.as_str().starts_with("https://ex.com/wp-json/?_fields="));
    assert_eq!(req.timeout, Duration::from_secs(7));
    assert_eq!(req.max_body_bytes, Some(4096));
    assert_eq!(req.headers["user-agent"], "probe/1");
    // `ada:s3cret`, Basic.
    assert_eq!(req.headers["authorization"], "Basic YWRhOnMzY3JldA==");
    assert!(req.body.is_none());
}

#[tokio::test]
async fn unreachable_is_retried_and_refused_is_not() {
    let (c, t) = client(
        Scripted::new(vec![
            Err(TransportError::Unreachable(boxed("connect"))),
            ok(json!({"name": "Ex"})),
        ]),
        1,
    );
    c.site_info().await.unwrap();
    assert_eq!(t.seen.lock().unwrap().len(), 2);

    let (c, t) = client(
        Scripted::new(vec![Err(TransportError::Refused(boxed("not on the list")))]),
        3,
    );
    let err = c.site_info().await.unwrap_err();
    assert!(matches!(
        err,
        Error::Transport(TransportError::Refused(_))
    ));
    assert_eq!(t.seen.lock().unwrap().len(), 1, "a refusal is final");
}

#[tokio::test]
async fn an_oversized_body_and_an_api_error_keep_their_meaning() {
    let (c, _) = client(
        Scripted::new(vec![Err(TransportError::BodyTooLarge {
            url: "https://ex.com/wp-json/".into(),
            limit: 4096,
        })]),
        0,
    );
    assert!(matches!(
        c.site_info().await.unwrap_err(),
        Error::BodyTooLarge { limit: 4096, .. }
    ));

    let (c, _) = client(
        Scripted::new(vec![Ok(TransportResponse {
            status: StatusCode::NOT_FOUND,
            headers: HeaderMap::new(),
            url: Url::parse("https://ex.com/wp-json/").unwrap(),
            body: br#"{"code":"rest_no_route","message":"No route","data":{"status":404}}"#
                .to_vec(),
        })]),
        0,
    );
    match c.site_info().await.unwrap_err() {
        Error::Api(api) => assert!(api.is_no_route()),
        other => panic!("{other:?}"),
    }
}
