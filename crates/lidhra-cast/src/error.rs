use thiserror::Error;

/// Everything that can go wrong discovering or driving a TV.
#[derive(Error, Debug)]
pub enum CastError {
    /// Socket-level failure (no network, multicast blocked, ...).
    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// The device answered with a non-2xx status that is not a SOAP fault.
    #[error("device answered HTTP {status}: {body}")]
    Status { status: u16, body: String },
    /// A UPnP renderer rejected an action (SOAP fault carrying a `UPnPError`).
    #[error("renderer rejected {action}: UPnP error {code} ({description})")]
    Soap { action: String, code: u16, description: String },
    /// The device description lists no AVTransport service, so it cannot play a URL.
    #[error("device has no AVTransport service (not a media renderer)")]
    NoAvTransport,
    /// The device record cannot be used (for example a malformed `location`).
    #[error("invalid device: {0}")]
    InvalidDevice(String),
    /// The media cannot be cast (for example not an http(s) URL).
    #[error("cannot cast this media: {0}")]
    InvalidMedia(String),
}

pub type Result<T> = std::result::Result<T, CastError>;
