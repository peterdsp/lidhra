//! Roku players and Roku TVs over the External Control Protocol (ECP), plain HTTP on port
//! 8060. Docs: <https://developer.roku.com/docs/developer-program/dev-tools/external-control-api.md>.
//!
//! - `GET /query/device-info` returns `<device-info>` with `user-device-name` (set by the
//!   user), `friendly-device-name`, `vendor-name` and `model-name`.
//! - Playing a URL uses the built-in "Play on Roku" channel (id 15985):
//!   `POST /input/15985?t=v&u=<url>&videoName=<title>&videoFormat=<fmt>`. `t=v` means video;
//!   `videoFormat` is one of Roku's stream formats (`mp4`, `mkv`, `hls`, `dash`, ...) and is
//!   left out when unknown so the player sniffs it.
//! - Stop sends `POST /keypress/Back`: it leaves the player and returns to the previous
//!   screen, which is what "stop casting" means to a viewer. `keypress/Home` would also
//!   close whatever channel the viewer was in before, so it is not used.
//!
//! ECP only answers on the LAN and only when the device's "Control by mobile apps" setting
//! allows it; a disabled setting shows up as HTTP 403.

use crate::error::{CastError, Result};
use crate::util::{extension, origin, percent_encode};
use crate::xml;
use crate::{Device, Media};

/// The "Play on Roku" system channel.
const PLAY_ON_ROKU: &str = "15985";

/// Name and model from `query/device-info`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Info {
    pub name: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
}

pub(crate) fn parse_device_info(body: &str) -> Info {
    let info = xml::element(body, "device-info").unwrap_or(body);
    let model = xml::text(info, "model-name").or_else(|| xml::text(info, "friendly-model-name"));
    let name = xml::text(info, "user-device-name")
        .or_else(|| xml::text(info, "friendly-device-name"))
        .or_else(|| model.clone());
    Info { name, manufacturer: xml::text(info, "vendor-name"), model }
}

/// `http://<ip>:8060/` from an SSDP LOCATION (always with one trailing slash).
pub(crate) fn base_url(location: &str) -> Option<String> {
    origin(location).map(|o| format!("{o}/"))
}

/// Roku `videoFormat` from the MIME type, else the URL's extension.
pub(crate) fn video_format(media: &Media) -> Option<&'static str> {
    let by_mime = media.mime.as_deref().map(|m| m.split(';').next().unwrap_or("").trim().to_ascii_lowercase());
    let from_mime = by_mime.as_deref().and_then(|m| match m {
        "video/mp4" | "video/quicktime" | "video/x-m4v" => Some("mp4"),
        "video/x-matroska" | "video/mkv" => Some("mkv"),
        "application/vnd.apple.mpegurl" | "application/x-mpegurl" | "audio/mpegurl" => Some("hls"),
        "application/dash+xml" => Some("dash"),
        _ => None,
    });
    from_mime.or_else(|| match extension(&media.url)?.as_str() {
        "mp4" | "m4v" | "mov" => Some("mp4"),
        "mkv" => Some("mkv"),
        "m3u8" => Some("hls"),
        "mpd" => Some("dash"),
        _ => None,
    })
}

/// Query string shared by the `input` and `launch` forms of the Play on Roku call.
fn play_query(media: &Media) -> String {
    let mut q = format!("t=v&u={}&videoName={}", percent_encode(&media.url), percent_encode(&media.title));
    if let Some(fmt) = video_format(media) {
        q.push_str("&videoFormat=");
        q.push_str(fmt);
    }
    q
}

/// `POST` target that hands `media` to the Play on Roku channel.
pub(crate) fn play_url(base: &str, media: &Media) -> String {
    format!("{base}input/{PLAY_ON_ROKU}?{}", play_query(media))
}

fn device_base(device: &Device) -> Result<String> {
    base_url(&device.location)
        .ok_or_else(|| CastError::InvalidDevice(format!("bad Roku location {:?}", device.location)))
}

pub(crate) async fn fetch_info(http: &reqwest::Client, base: &str) -> Result<Info> {
    let resp = http.get(format!("{base}query/device-info")).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(CastError::Status { status: status.as_u16(), body });
    }
    Ok(parse_device_info(&body))
}

/// ECP commands are bodiless POSTs; send an explicit zero length (older firmware wants it).
async fn post(http: &reqwest::Client, url: &str) -> Result<()> {
    let resp = http.post(url).header("Content-Length", "0").body("").send().await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    Err(CastError::Status { status: status.as_u16(), body })
}

pub(crate) async fn play(http: &reqwest::Client, device: &Device, media: &Media) -> Result<()> {
    let base = device_base(device)?;
    match post(http, &play_url(&base, media)).await {
        // 401/403: mobile control disabled, retrying another endpoint will not help.
        Err(CastError::Status { status, .. }) if status != 401 && status != 403 => {
            // Firmware that does not route `input/15985` launches the channel directly.
            post(http, &format!("{base}launch/{PLAY_ON_ROKU}?{}", play_query(media))).await
        }
        other => other,
    }
}

pub(crate) async fn stop(http: &reqwest::Client, device: &Device) -> Result<()> {
    post(http, &format!("{}keypress/Back", device_base(device)?)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(url: &str, mime: Option<&str>) -> Media {
        Media { url: url.into(), title: "Big Buck Bunny & Friends".into(), mime: mime.map(String::from) }
    }

    #[test]
    fn parses_device_info() {
        let info = parse_device_info(include_str!("fixtures/roku_device_info.xml"));
        assert_eq!(info.name.as_deref(), Some("Living Room & Den"));
        assert_eq!(info.manufacturer.as_deref(), Some("Roku"));
        assert_eq!(info.model.as_deref(), Some("Roku Ultra"));
    }

    #[test]
    fn device_info_name_fallbacks() {
        let friendly = "<device-info><friendly-device-name>Bedroom TV</friendly-device-name>\
                        <model-name>TCL 55S425</model-name></device-info>";
        assert_eq!(parse_device_info(friendly).name.as_deref(), Some("Bedroom TV"));
        let model_only =
            "<device-info><user-device-name></user-device-name><model-name>Roku Express</model-name></device-info>";
        assert_eq!(parse_device_info(model_only).name.as_deref(), Some("Roku Express"));
    }

    #[test]
    fn builds_base_url() {
        assert_eq!(base_url("http://192.168.1.44:8060/").as_deref(), Some("http://192.168.1.44:8060/"));
        assert_eq!(base_url("http://192.168.1.44:8060").as_deref(), Some("http://192.168.1.44:8060/"));
        assert_eq!(base_url("garbage"), None);
    }

    #[test]
    fn builds_play_on_roku_url() {
        let m = media("https://dl.example/d/AB12/Big Buck Bunny.mkv?token=a&b=1", None);
        assert_eq!(
            play_url("http://192.168.1.44:8060/", &m),
            "http://192.168.1.44:8060/input/15985?t=v\
             &u=https%3A%2F%2Fdl.example%2Fd%2FAB12%2FBig%20Buck%20Bunny.mkv%3Ftoken%3Da%26b%3D1\
             &videoName=Big%20Buck%20Bunny%20%26%20Friends&videoFormat=mkv"
        );
        let unknown = media("https://dl.example/d/AB12", None);
        assert!(!play_url("http://r:8060/", &unknown).contains("videoFormat"));
    }

    #[test]
    fn infers_video_format() {
        assert_eq!(video_format(&media("https://x/a", Some("video/mp4"))), Some("mp4"));
        assert_eq!(video_format(&media("https://x/a", Some("Video/X-Matroska; codecs=x"))), Some("mkv"));
        assert_eq!(video_format(&media("https://x/a", Some("application/vnd.apple.mpegurl"))), Some("hls"));
        // Unhelpful MIME falls back to the extension.
        assert_eq!(video_format(&media("https://x/movie.M4V?x=1", Some("application/octet-stream"))), Some("mp4"));
        assert_eq!(video_format(&media("https://x/live/index.m3u8", None)), Some("hls"));
        assert_eq!(video_format(&media("https://x/manifest.mpd", None)), Some("dash"));
        assert_eq!(video_format(&media("https://x/clip.avi", None)), None);
        assert_eq!(video_format(&media("https://x/download", None)), None);
    }
}
