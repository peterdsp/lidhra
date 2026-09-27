//! SSDP discovery (UPnP Device Architecture 1.1, section 1.3.2).
//!
//! A control point multicasts `M-SEARCH * HTTP/1.1` to `239.255.255.250:1900` with a search
//! target (`ST`) and a max wait (`MX`, seconds). Every matching device answers by *unicast*
//! to the sender's port with an HTTP-style `200 OK` whose headers include `LOCATION` (where
//! to fetch its description), `ST` (echo of the target) and `USN` (unique service name,
//! `uuid:<device>::<type>`). Header names are case-insensitive and vary in the wild.
//!
//! Roku players answer the non-standard target `roku:ecp` with `LOCATION: http://<ip>:8060/`.

use crate::error::Result;
use crate::Kind;
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::{sleep, timeout_at, Instant};

const MULTICAST: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)), 1900);
/// UPnP renderers (TVs, AV receivers, speakers) that implement AVTransport.
pub(crate) const ST_RENDERER: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
/// Roku External Control Protocol.
pub(crate) const ST_ROKU: &str = "roku:ecp";
/// Seconds a device may wait before answering (spreads replies out).
const MX: u8 = 2;
/// UPnP 1.1 recommends TTL 2 so searches stay on the local network.
const TTL: u32 = 2;

/// The `M-SEARCH` request for one search target.
pub(crate) fn msearch(st: &str) -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: {MX}\r\nST: {st}\r\n\
         USER-AGENT: Lidhra/{} UPnP/1.1 lidhra-cast\r\n\r\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// The headers of one search response that discovery cares about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Response {
    pub location: String,
    pub st: String,
    pub usn: String,
    pub server: Option<String>,
}

impl Response {
    /// Which protocol this answer is for, from its `ST`; `None` for targets we did not ask for.
    pub(crate) fn kind(&self) -> Option<Kind> {
        let st = self.st.to_ascii_lowercase();
        if st == ST_ROKU {
            Some(Kind::Roku)
        } else if st.contains("device:mediarenderer:") {
            Some(Kind::Dlna)
        } else {
            None
        }
    }

    /// Stable device id: the `uuid:...` part of the USN (before `::<type>`), else LOCATION.
    pub(crate) fn device_id(&self) -> String {
        match self.usn.split("::").next().map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => id.to_string(),
            None => self.location.clone(),
        }
    }
}

/// Parse one datagram. Only `200` responses with a `LOCATION` are useful.
pub(crate) fn parse_response(raw: &str) -> Option<Response> {
    let mut lines = raw.lines();
    let status = lines.next()?.trim();
    if !status.starts_with("HTTP/") || status.split_whitespace().nth(1) != Some("200") {
        return None;
    }
    let (mut location, mut st, mut usn, mut server) = (None, None, None, None);
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim().to_string();
        match name.trim().to_ascii_uppercase().as_str() {
            "LOCATION" => location = Some(value),
            "ST" => st = Some(value),
            "USN" => usn = Some(value),
            "SERVER" => server = Some(value),
            _ => {}
        }
    }
    Some(Response {
        location: location.filter(|l| !l.is_empty())?,
        st: st.unwrap_or_default(),
        usn: usn.unwrap_or_default(),
        server: server.filter(|s| !s.is_empty()),
    })
}

/// Drops repeated answers: devices reply once per search (we send each twice) and some
/// reply several times. Keyed by kind + USN device id, and by kind + LOCATION as a fallback.
#[derive(Default)]
pub(crate) struct Dedupe {
    seen: HashSet<String>,
}

impl Dedupe {
    /// `true` the first time a device is seen.
    pub(crate) fn insert(&mut self, r: &Response, kind: Kind) -> bool {
        let by_id = format!("{kind:?}|id|{}", r.device_id());
        let by_location = format!("{kind:?}|loc|{}", r.location.trim_end_matches('/'));
        let fresh = !self.seen.contains(&by_id) && !self.seen.contains(&by_location);
        self.seen.insert(by_id);
        self.seen.insert(by_location);
        fresh
    }
}

/// Multicast the searches (each twice, UDP is lossy) and hand every parsed response and
/// its sender address to `on_response` until `wait` elapses.
pub(crate) async fn search(wait: Duration, mut on_response: impl FnMut(Response, SocketAddr)) -> Result<()> {
    let deadline = Instant::now() + wait;
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    sock.set_multicast_ttl_v4(TTL)?;
    let requests = [msearch(ST_RENDERER), msearch(ST_ROKU)];
    for req in &requests {
        sock.send_to(req.as_bytes(), MULTICAST).await?;
    }
    sleep(Duration::from_millis(150).min(wait)).await;
    for req in &requests {
        // The first round proved the route works; a lost repeat is not an error.
        let _ = sock.send_to(req.as_bytes(), MULTICAST).await;
    }
    let mut buf = [0u8; 2048];
    while let Ok(received) = timeout_at(deadline, sock.recv_from(&mut buf)).await {
        let Ok((n, from)) = received else { continue };
        if let Some(resp) = parse_response(&String::from_utf8_lossy(&buf[..n])) {
            on_response(resp, from);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMSUNG: &str = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age = 1800\r\nDATE: Thu, 01 Jan 1970 00:08:10 GMT\r\n\
        EXT:\r\nLOCATION: http://192.168.1.20:9197/dmr\r\nSERVER: SHP, UPnP/1.0, Samsung UPnP SDK/1.0\r\n\
        ST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\
        USN: uuid:08f0d180-0096-1000-a8ec-f47b5e6b2c12::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\
        Content-Length: 0\r\n\r\n";

    const LG: &str = "HTTP/1.1 200 OK\r\nCache-Control: max-age=1800\r\nDate: Sat, 27 Sep 2026 10:00:00 GMT\r\n\
        Ext: \r\n\
        Location: http://192.168.1.31:1161/\r\n\
        Server: Linux/4.4.84-202.jaguar-trunk UPnP/1.0 LGE WebOS TV/Version 0.9\r\n\
        St: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\
        Usn: uuid:9ab0d5e4-3c21-4f8b-b6e2-7d1f4a2c9e10::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\
        DLNADeviceName.lge.com: %5bLG%5d%20webOS%20TV%20OLED55C1PUB\r\n\r\n";

    const ROKU: &str = "HTTP/1.1 200 OK\r\nCache-Control: max-age=3600\r\nST: roku:ecp\r\n\
        USN: uuid:roku:ecp:X00400AB1234\r\nExt: \r\nServer: Roku/12.5.0 UPnP/1.0 Roku/12.5.0\r\n\
        LOCATION: http://192.168.1.44:8060/\r\ndevice-group.roku.com: 5B8A0F6E1D2C3B4A\r\n\r\n";

    #[test]
    fn builds_msearch() {
        let m = msearch(ST_ROKU);
        assert!(m.starts_with("M-SEARCH * HTTP/1.1\r\n"));
        assert!(m.contains("MAN: \"ssdp:discover\"\r\n"));
        assert!(m.contains("MX: 2\r\n"));
        assert!(m.contains("ST: roku:ecp\r\n"));
        assert!(m.ends_with("\r\n\r\n"));
    }

    #[test]
    fn parses_samsung_response() {
        let r = parse_response(SAMSUNG).unwrap();
        assert_eq!(r.location, "http://192.168.1.20:9197/dmr");
        assert_eq!(r.server.as_deref(), Some("SHP, UPnP/1.0, Samsung UPnP SDK/1.0"));
        assert_eq!(r.kind(), Some(Kind::Dlna));
        assert_eq!(r.device_id(), "uuid:08f0d180-0096-1000-a8ec-f47b5e6b2c12");
    }

    #[test]
    fn parses_mixed_case_lg_response() {
        let r = parse_response(LG).unwrap();
        assert_eq!(r.location, "http://192.168.1.31:1161/");
        assert_eq!(r.st, ST_RENDERER);
        assert_eq!(r.kind(), Some(Kind::Dlna));
        assert_eq!(r.device_id(), "uuid:9ab0d5e4-3c21-4f8b-b6e2-7d1f4a2c9e10");
    }

    #[test]
    fn parses_roku_response() {
        let r = parse_response(ROKU).unwrap();
        assert_eq!(r.location, "http://192.168.1.44:8060/");
        assert_eq!(r.kind(), Some(Kind::Roku));
        assert_eq!(r.device_id(), "uuid:roku:ecp:X00400AB1234");
    }

    #[test]
    fn rejects_non_responses_and_foreign_targets() {
        assert_eq!(parse_response("NOTIFY * HTTP/1.1\r\nLOCATION: http://x/\r\n\r\n"), None);
        assert_eq!(parse_response("HTTP/1.1 404 Not Found\r\nLOCATION: http://x/\r\n\r\n"), None);
        assert_eq!(parse_response("HTTP/1.1 200 OK\r\nST: roku:ecp\r\n\r\n"), None);
        let other = parse_response("HTTP/1.1 200 OK\r\nLOCATION: http://x/\r\nST: upnp:rootdevice\r\n\r\n").unwrap();
        assert_eq!(other.kind(), None);
        // No USN: the location doubles as the id.
        assert_eq!(other.device_id(), "http://x/");
    }

    #[test]
    fn dedupes_by_usn_and_location() {
        let mut d = Dedupe::default();
        let samsung = parse_response(SAMSUNG).unwrap();
        assert!(d.insert(&samsung, Kind::Dlna));
        assert!(!d.insert(&samsung, Kind::Dlna), "second reply to the repeated search");

        // Same device id at a new port (after a TV restart), still one device.
        let moved = Response { location: "http://192.168.1.20:9198/dmr".into(), ..samsung.clone() };
        assert!(!d.insert(&moved, Kind::Dlna));

        // Different USN but the same description URL (some stacks vary the USN suffix).
        let alias = Response { usn: "uuid:other".into(), location: "http://192.168.1.20:9197/dmr/".into(), ..samsung };
        assert!(!d.insert(&alias, Kind::Dlna));

        let roku = parse_response(ROKU).unwrap();
        assert!(d.insert(&roku, Kind::Roku));
        let lg = parse_response(LG).unwrap();
        assert!(d.insert(&lg, Kind::Dlna));
    }
}
