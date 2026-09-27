//! DLNA / UPnP AV renderers: device description parsing and the AVTransport SOAP calls.
//!
//! The SSDP `LOCATION` points at a device description (`<root><device>...`). Its root
//! `<device>` carries `friendlyName` / `manufacturer` / `modelName`; embedded devices live
//! in `<deviceList>` and must not override them. The renderer's `AVTransport` service may
//! sit on the root or on an embedded device, so every `<service>` is scanned by
//! `serviceType`. Its `controlURL` may be absolute or relative; relative URLs resolve
//! against `<URLBase>` when present, else against the description URL.
//!
//! Playback is two SOAP actions on that control URL: `SetAVTransportURI` (the stream URL
//! plus DIDL-Lite metadata, which many TVs require to pick a player) then `Play`. The TV
//! fetches the URL itself. Errors come back as HTTP 500 with a SOAP `Fault` whose
//! `detail/UPnPError` has a numeric `errorCode` and an `errorDescription`.

use crate::error::{CastError, Result};
use crate::util::{guess_mime, resolve};
use crate::xml;
use crate::{Device, Media};
use std::fmt::Write;
use std::time::Duration;

const AV_TRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:1";

/// What discovery keeps from a device description.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Description {
    pub name: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    /// Absolute AVTransport control URL; `None` when the device has no AVTransport.
    pub control_url: Option<String>,
}

/// Parse a device description fetched from `location`.
pub(crate) fn parse_description(body: &str, location: &str) -> Description {
    let root = xml::element(body, "root").unwrap_or(body);
    let base = xml::text(root, "URLBase").unwrap_or_else(|| location.to_string());
    // The first <device> is the root device (the depth-aware match spans its embedded ones).
    let device = xml::element(root, "device").unwrap_or(root);
    let own = xml::without(device, "deviceList");
    let field = |name: &str| xml::text(&own, name).or_else(|| xml::text(device, name));
    let control_url = xml::elements(body, "service")
        .into_iter()
        .find(|s| xml::text(s, "serviceType").is_some_and(|t| t.contains("AVTransport")))
        .and_then(|s| xml::text(s, "controlURL"))
        .map(|c| resolve(&base, &c));
    Description {
        name: field("friendlyName"),
        manufacturer: field("manufacturer"),
        model: field("modelName"),
        control_url,
    }
}

/// GET and parse the description at `location`.
pub(crate) async fn fetch_description(http: &reqwest::Client, location: &str) -> Result<Description> {
    let resp = http.get(location).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(CastError::Status { status: status.as_u16(), body: truncate(body) });
    }
    Ok(parse_description(&body, location))
}

/// DIDL-Lite metadata for one video item (itself XML; the envelope escapes it again).
pub(crate) fn didl_lite(title: &str, url: &str, mime: &str) -> String {
    format!(
        "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\">\
         <item id=\"0\" parentID=\"-1\" restricted=\"1\"><dc:title>{}</dc:title>\
         <upnp:class>object.item.videoItem</upnp:class>\
         <res protocolInfo=\"http-get:*:{}:*\">{}</res></item></DIDL-Lite>",
        xml::escape(title),
        xml::escape(mime),
        xml::escape(url)
    )
}

/// SOAP envelope for an AVTransport action; argument values are escaped here, in order.
pub(crate) fn soap_envelope(action: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>",
    );
    let _ = write!(out, "<u:{action} xmlns:u=\"{AV_TRANSPORT}\">");
    for (name, value) in args {
        xml::push_element(&mut out, name, value);
    }
    let _ = write!(out, "</u:{action}></s:Body></s:Envelope>");
    out
}

/// `(errorCode, errorDescription)` from a SOAP fault body; `None` if it is not a fault.
pub(crate) fn parse_fault(body: &str) -> Option<(u16, String)> {
    let fault = xml::element(body, "Fault")?;
    let code = xml::text(fault, "errorCode").and_then(|c| c.parse().ok()).unwrap_or(0);
    let description = xml::text(fault, "errorDescription")
        .or_else(|| known_error(code).map(String::from))
        .or_else(|| xml::text(fault, "faultstring"))
        .unwrap_or_default();
    Some((code, description))
}

/// Standard AVTransport error codes, for renderers that send a bare code.
fn known_error(code: u16) -> Option<&'static str> {
    Some(match code {
        401 => "Invalid action",
        402 => "Invalid args",
        501 => "Action failed",
        701 => "Transition not available",
        702 => "No contents",
        705 => "Transport is locked",
        714 => "Illegal MIME-type",
        716 => "Resource not found",
        718 => "Invalid InstanceID",
        _ => return None,
    })
}

fn truncate(mut body: String) -> String {
    if body.len() > 300 {
        let cut = (0..=300).rev().find(|&i| body.is_char_boundary(i)).unwrap_or(0);
        body.truncate(cut);
        body.push_str("...");
    }
    body
}

/// POST one action; SOAP faults become [`CastError::Soap`].
async fn invoke(http: &reqwest::Client, control_url: &str, action: &str, args: &[(&str, &str)]) -> Result<()> {
    let resp = http
        .post(control_url)
        .header("SOAPACTION", format!("\"{AV_TRANSPORT}#{action}\""))
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .body(soap_envelope(action, args))
        .send()
        .await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    Err(match parse_fault(&body) {
        Some((code, description)) => CastError::Soap { action: action.to_string(), code, description },
        None => CastError::Status { status: status.as_u16(), body: truncate(body) },
    })
}

/// The stored control URL, or re-read the description (devices rebuilt without one).
async fn control_url(http: &reqwest::Client, device: &Device) -> Result<String> {
    if let Some(url) = device.control_url.as_deref().filter(|u| !u.is_empty()) {
        return Ok(url.to_string());
    }
    fetch_description(http, &device.location).await?.control_url.ok_or(CastError::NoAvTransport)
}

pub(crate) async fn play(http: &reqwest::Client, device: &Device, media: &Media) -> Result<()> {
    let control = control_url(http, device).await?;
    let mime = media.mime.as_deref().filter(|m| !m.is_empty()).or_else(|| guess_mime(&media.url)).unwrap_or("*");
    let meta = didl_lite(&media.title, &media.url, mime);
    let set = [("InstanceID", "0"), ("CurrentURI", media.url.as_str()), ("CurrentURIMetaData", meta.as_str())];
    if let Err(e) = invoke(http, &control, "SetAVTransportURI", &set).await {
        // A renderer that is already playing may refuse a new URI (705 on some Samsungs):
        // stop it and try once more. Transport errors are returned as-is.
        if !matches!(e, CastError::Soap { .. }) {
            return Err(e);
        }
        let _ = invoke(http, &control, "Stop", &[("InstanceID", "0")]).await;
        invoke(http, &control, "SetAVTransportURI", &set).await?;
    }
    let play = [("InstanceID", "0"), ("Speed", "1")];
    match invoke(http, &control, "Play", &play).await {
        // Some TVs (LG) answer 701 while still loading the new URI; give them a moment.
        Err(CastError::Soap { .. }) => {
            tokio::time::sleep(Duration::from_secs(1)).await;
            invoke(http, &control, "Play", &play).await
        }
        other => other,
    }
}

pub(crate) async fn stop(http: &reqwest::Client, device: &Device) -> Result<()> {
    let control = control_url(http, device).await?;
    invoke(http, &control, "Stop", &[("InstanceID", "0")]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_samsung_description() {
        let d = parse_description(include_str!("fixtures/samsung_tv.xml"), "http://192.168.1.20:9197/dmr");
        assert_eq!(d.name.as_deref(), Some("[TV] Samsung Q60 Series (55)"));
        assert_eq!(d.manufacturer.as_deref(), Some("Samsung Electronics"));
        assert_eq!(d.model.as_deref(), Some("QE55Q60RATXXU"));
        // Third service, absolute path: resolves against the LOCATION origin.
        assert_eq!(d.control_url.as_deref(), Some("http://192.168.1.20:9197/upnp/control/AVTransport1"));
    }

    #[test]
    fn root_fields_win_over_embedded_device_and_relative_control_url_resolves() {
        let location = "http://192.168.1.40:49154/description.xml";
        let d = parse_description(include_str!("fixtures/embedded_renderer.xml"), location);
        assert_eq!(d.name.as_deref(), Some("Living Room AVR"));
        assert_eq!(d.manufacturer.as_deref(), Some("Yamaha Corporation"));
        assert_eq!(d.model.as_deref(), Some("RX-V685"));
        // AVTransport is the embedded device's second service, not the first service overall.
        assert_eq!(d.control_url.as_deref(), Some("http://192.168.1.40:49154/AVTransport/ctrl"));
    }

    #[test]
    fn url_base_overrides_location() {
        let d = parse_description(include_str!("fixtures/lg_urlbase.xml"), "http://192.168.1.31:1161/");
        assert_eq!(d.name.as_deref(), Some("[LG] webOS TV OLED55C1PUB"));
        assert_eq!(d.manufacturer.as_deref(), Some("LG Electronics"));
        assert_eq!(
            d.control_url.as_deref(),
            Some("http://192.168.1.31:1162/AVTransport/9ab0d5e4-3c21-4f8b-b6e2-7d1f4a2c9e10/control.xml")
        );
    }

    #[test]
    fn description_without_av_transport_has_no_control_url() {
        let xml = "<root><device><friendlyName>NAS</friendlyName><serviceList><service>\
                   <serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType>\
                   <controlURL>/cd</controlURL></service></serviceList></device></root>";
        let d = parse_description(xml, "http://10.0.0.2/desc.xml");
        assert_eq!(d.name.as_deref(), Some("NAS"));
        assert_eq!(d.control_url, None);
    }

    #[test]
    fn prefixed_description_tags_are_read() {
        let xml = "<u:root xmlns:u=\"urn:schemas-upnp-org:device-1-0\"><u:device>\
                   <u:friendlyName>Box &amp; Co</u:friendlyName><u:serviceList><u:service>\
                   <u:serviceType>urn:schemas-upnp-org:service:AVTransport:1</u:serviceType>\
                   <u:controlURL>http://10.0.0.3:8080/av</u:controlURL>\
                   </u:service></u:serviceList></u:device></u:root>";
        let d = parse_description(xml, "http://10.0.0.3/d.xml");
        assert_eq!(d.name.as_deref(), Some("Box & Co"));
        assert_eq!(d.control_url.as_deref(), Some("http://10.0.0.3:8080/av"));
    }

    #[test]
    fn didl_lite_escapes_title_and_url() {
        let didl = didl_lite("Tom & Jerry <\"The Movie\">", "https://dl.example/f.mkv?a=1&b=2", "video/x-matroska");
        assert!(didl.contains("<dc:title>Tom &amp; Jerry &lt;&quot;The Movie&quot;&gt;</dc:title>"));
        let res = "<res protocolInfo=\"http-get:*:video/x-matroska:*\">https://dl.example/f.mkv?a=1&amp;b=2</res>";
        assert!(didl.contains(res));
        assert!(didl.contains("<upnp:class>object.item.videoItem</upnp:class>"));
        // It is well-formed enough for our own reader to round-trip.
        assert_eq!(xml::text(&didl, "title").as_deref(), Some("Tom & Jerry <\"The Movie\">"));
    }

    #[test]
    fn soap_envelope_double_escapes_metadata() {
        let url = "https://dl.example/f.mkv?a=1&b=2";
        let meta = didl_lite("A & B", url, "video/mp4");
        let args = [("InstanceID", "0"), ("CurrentURI", url), ("CurrentURIMetaData", meta.as_str())];
        let env = soap_envelope("SetAVTransportURI", &args);
        assert!(env.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>"));
        assert!(env.contains("<u:SetAVTransportURI xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">"));
        assert!(env.contains("<InstanceID>0</InstanceID><CurrentURI>https://dl.example/f.mkv?a=1&amp;b=2</CurrentURI>"));
        assert!(env.contains("<CurrentURIMetaData>&lt;DIDL-Lite "));
        assert!(env.contains("&lt;dc:title&gt;A &amp;amp; B&lt;/dc:title&gt;"));
        assert!(env.contains("?a=1&amp;amp;b=2&lt;/res&gt;"));
        // Decoding the envelope once yields the original DIDL-Lite document.
        assert_eq!(xml::text(&env, "CurrentURIMetaData").as_deref(), Some(meta.as_str()));
        assert!(env.ends_with("</u:SetAVTransportURI></s:Body></s:Envelope>"));
    }

    #[test]
    fn play_and_stop_envelopes() {
        let ns = "xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\"";
        let env = soap_envelope("Play", &[("InstanceID", "0"), ("Speed", "1")]);
        assert!(env.contains(&format!("<u:Play {ns}><InstanceID>0</InstanceID><Speed>1</Speed></u:Play>")));
        let env = soap_envelope("Stop", &[("InstanceID", "0")]);
        assert!(env.contains(&format!("<u:Stop {ns}><InstanceID>0</InstanceID></u:Stop>")));
    }

    #[test]
    fn parses_soap_faults() {
        let body = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <s:Fault>
      <faultcode>s:Client</faultcode>
      <faultstring>UPnPError</faultstring>
      <detail>
        <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
          <errorCode>714</errorCode>
          <errorDescription>Illegal MIME-type</errorDescription>
        </UPnPError>
      </detail>
    </s:Fault>
  </s:Body>
</s:Envelope>"#;
        assert_eq!(parse_fault(body), Some((714, "Illegal MIME-type".to_string())));

        let bare = "<s:Envelope><s:Body><s:Fault><faultstring>UPnPError</faultstring><detail><UPnPError>\
                    <errorCode>701</errorCode></UPnPError></detail></s:Fault></s:Body></s:Envelope>";
        assert_eq!(parse_fault(bare), Some((701, "Transition not available".to_string())));

        let no_detail = "<Envelope><Body><Fault><faultstring>Server busy</faultstring></Fault></Body></Envelope>";
        assert_eq!(parse_fault(no_detail), Some((0, "Server busy".to_string())));

        assert_eq!(parse_fault("<html>Internal Server Error</html>"), None);
    }

    #[test]
    fn truncates_long_bodies_on_char_boundaries() {
        let long = "é".repeat(400);
        let t = truncate(long);
        assert!(t.ends_with("...") && t.len() <= 303);
        assert_eq!(truncate("short".into()), "short");
    }
}
