#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Client(#[from] tec_client::Error),

    #[error("could not write output: {0}")]
    Io(#[from] std::io::Error),

    #[error("could not encode output: {0}")]
    Json(#[from] serde_json::Error),

    #[error("unknown time zone {0:?}")]
    TimeZone(String, #[source] tec_client::jiff::Error),

    #[error("{0}")]
    Usage(String),
}
