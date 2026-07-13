use thiserror::Error;

/// Everything that can go wrong talking to a debrid provider.
#[derive(Error, Debug)]
pub enum DebridError {
    #[error("authentication failed or token invalid")]
    Auth,
    #[error("provider rate limited the request")]
    RateLimited,
    #[error("hash is not cached and could not be added")]
    NotCached,
    #[error("transfer failed on the provider side: {0}")]
    TransferFailed(String),
    #[error("could not parse magnet / info-hash: {0}")]
    BadMagnet(String),
    #[error("provider returned an error: {0}")]
    Provider(String),
    #[error("unexpected response shape: {0}")]
    Decode(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, DebridError>;
