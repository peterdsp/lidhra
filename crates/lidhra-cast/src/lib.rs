//! # lidhra-cast
//!
//! Cast a direct video URL (typically an HTTPS link already resolved by a debrid provider)
//! to a TV on the same LAN that cannot run a Lidhra app:
//!
//! - **DLNA / UPnP AV renderers** (`MediaRenderer:1` with an `AVTransport` service): most
//!   Samsung, LG, Sony, Panasonic and Philips TVs, many AV receivers and soundbars.
//! - **Roku** players and Roku TVs, through the External Control Protocol and the built-in
//!   "Play on Roku" channel.
//!
//! The TV fetches the URL itself, so it must be reachable from the TV and must not need
//! custom headers or cookies. Chromecast (Cast v2: protobuf over TLS) and AirPlay are
//! intentionally out of scope for this crate.
//!
//! ```no_run
//! # async fn demo() -> lidhra_cast::Result<()> {
//! use std::time::Duration;
//! let devices = lidhra_cast::discover(Duration::from_secs(3)).await?;
//! let media = lidhra_cast::Media::new("https://example.com/movie.mkv", "Movie");
//! lidhra_cast::play(&devices[0], &media).await?;
//! # Ok(()) }
//! ```

mod dlna;
mod error;
mod roku;
mod ssdp;
mod util;
mod xml;

pub use error::{CastError, Result};

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::task::JoinSet;

/// How a device is driven.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// UPnP AV / DLNA renderer, controlled with AVTransport SOAP actions.
    Dlna,
    /// Roku External Control Protocol (port 8060).
    Roku,
}

/// A castable TV found on the LAN. Serializable so a UI can list devices and hand one back
/// to [`play`] later without rediscovering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// Stable id from SSDP: the `uuid:...` part of the USN (the LOCATION when there is none).
    pub id: String,
    /// Human-readable name (the TV's friendly name when it reports one).
    pub name: String,
    pub kind: Kind,
    /// DLNA: the device description URL. Roku: the ECP base URL (`http://<ip>:8060/`).
    pub location: String,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// DLNA only: absolute AVTransport control URL. When missing, [`play`] and [`stop`]
    /// re-read it from `location`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_url: Option<String>,
}

/// What to play.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Media {
    /// Direct `http(s)` URL the TV can fetch on its own.
    pub url: String,
    /// Title shown by the TV's player.
    pub title: String,
    /// MIME type, e.g. `video/mp4`. Guessed from the URL extension when `None`.
    #[serde(default)]
    pub mime: Option<String>,
}

impl Media {
    pub fn new(url: impl Into<String>, title: impl Into<String>) -> Self {
        Media { url: url.into(), title: title.into(), mime: None }
    }
}

/// Search the LAN for DLNA renderers and Roku devices.
///
/// Listens for SSDP answers for `timeout`, then waits for the per-device lookups started
/// along the way (description or `device-info` fetch, each capped at a few seconds).
/// Devices whose description lists no AVTransport service are skipped; a device whose
/// description could not be fetched is kept with a fallback name. Sorted by name.
pub async fn discover(timeout: Duration) -> Result<Vec<Device>> {
    let http = util::http_client()?;
    let mut seen = ssdp::Dedupe::default();
    let mut lookups = JoinSet::new();
    ssdp::search(timeout, |resp, from| {
        let Some(kind) = resp.kind() else { return };
        if !answered_by(&resp.location, from) || !seen.insert(&resp, kind) {
            return;
        }
        let http = http.clone();
        lookups.spawn(async move { lookup(&http, resp, kind).await });
    })
    .await?;
    let mut devices = Vec::new();
    while let Some(done) = lookups.join_next().await {
        if let Ok(Some(device)) = done {
            devices.push(device);
        }
    }
    devices.sort_by_key(|d| (d.name.to_lowercase(), d.id.clone()));
    Ok(devices)
}

/// Start playing `media` on `device`, replacing whatever it was playing.
pub async fn play(device: &Device, media: &Media) -> Result<()> {
    let scheme = media.url.split_once("://").map(|(s, _)| s.to_ascii_lowercase());
    if !matches!(scheme.as_deref(), Some("http" | "https")) {
        return Err(CastError::InvalidMedia(format!("not an http(s) URL: {:?}", media.url)));
    }
    let http = util::http_client()?;
    match device.kind {
        Kind::Dlna => dlna::play(&http, device, media).await,
        Kind::Roku => roku::play(&http, device, media).await,
    }
}

/// Stop playback on `device` (DLNA `Stop`; Roku `Back` key, see the crate README).
pub async fn stop(device: &Device) -> Result<()> {
    let http = util::http_client()?;
    match device.kind {
        Kind::Dlna => dlna::stop(&http, device).await,
        Kind::Roku => roku::stop(&http, device).await,
    }
}

/// Only follow a LOCATION on the host that sent the answer, so a spoofed SSDP reply cannot
/// point discovery at another machine (or at a service on localhost).
fn answered_by(location: &str, from: SocketAddr) -> bool {
    util::host(location).and_then(|h| h.parse().ok()) == Some(from.ip())
}

/// Turn one SSDP answer into a [`Device`] by fetching what the protocol needs.
async fn lookup(http: &reqwest::Client, resp: ssdp::Response, kind: Kind) -> Option<Device> {
    let host = util::host(&resp.location).unwrap_or_default().to_string();
    let id = resp.device_id();
    match kind {
        Kind::Dlna => {
            let (name, manufacturer, model, control_url) = match dlna::fetch_description(http, &resp.location).await {
                // Answered the renderer search but cannot take a URL: not useful here.
                Ok(d) if d.control_url.is_none() => return None,
                Ok(d) => (d.name, d.manufacturer, d.model, d.control_url),
                Err(_) => (None, None, None, None),
            };
            let name = name.unwrap_or_else(|| format!("Media renderer ({host})"));
            Some(Device { id, name, kind, location: resp.location, manufacturer, model, control_url })
        }
        Kind::Roku => {
            let base = roku::base_url(&resp.location)?;
            let info = roku::fetch_info(http, &base).await.unwrap_or_default();
            let name = info.name.unwrap_or_else(|| format!("Roku ({host})"));
            let manufacturer = info.manufacturer.or_else(|| Some("Roku".to_string()));
            Some(Device { id, name, kind, location: base, manufacturer, model: info.model, control_url: None })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_round_trips_through_serde() {
        let d = Device {
            id: "uuid:08f0d180-0096-1000-a8ec-f47b5e6b2c12".into(),
            name: "[TV] Samsung Q60 Series (55)".into(),
            kind: Kind::Dlna,
            location: "http://192.168.1.20:9197/dmr".into(),
            manufacturer: Some("Samsung Electronics".into()),
            model: None,
            control_url: Some("http://192.168.1.20:9197/upnp/control/AVTransport1".into()),
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"kind\":\"dlna\""));
        let back: Device = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn minimal_device_record_deserializes() {
        let json = r#"{"id":"r","name":"Roku","kind":"roku","location":"http://10.0.0.9:8060/"}"#;
        let d: Device = serde_json::from_str(json).unwrap();
        assert_eq!(d.kind, Kind::Roku);
        assert_eq!(d.control_url, None);
    }

    #[test]
    fn only_trusts_location_on_the_answering_host() {
        let from: SocketAddr = "192.168.1.20:1900".parse().unwrap();
        assert!(answered_by("http://192.168.1.20:9197/dmr", from));
        assert!(!answered_by("http://127.0.0.1:8080/", from));
        assert!(!answered_by("http://tv.local:9197/dmr", from));
    }

    #[test]
    fn public_futures_are_send() {
        fn is_send<T: Send>(_: T) {}
        let d = Device {
            id: "x".into(),
            name: "x".into(),
            kind: Kind::Dlna,
            location: "http://10.0.0.9/d.xml".into(),
            manufacturer: None,
            model: None,
            control_url: None,
        };
        let m = Media::new("https://x/a.mp4", "a");
        is_send(discover(Duration::ZERO));
        is_send(play(&d, &m));
        is_send(stop(&d));
    }

    #[tokio::test]
    async fn play_rejects_non_http_urls() {
        let d = Device {
            id: "x".into(),
            name: "x".into(),
            kind: Kind::Roku,
            location: "http://10.0.0.9:8060/".into(),
            manufacturer: None,
            model: None,
            control_url: None,
        };
        let err = play(&d, &Media::new("file:///etc/passwd", "x")).await.unwrap_err();
        assert!(matches!(err, CastError::InvalidMedia(_)));
    }
}
