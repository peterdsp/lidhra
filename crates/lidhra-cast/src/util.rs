//! Small URL and media helpers (no extra deps).

use crate::error::Result;
use std::fmt::Write;
use std::time::Duration;

/// Per-request cap for LAN calls. TVs answer in milliseconds or not at all.
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// HTTP client for talking to TVs. Proxies are ignored: a system proxy cannot reach the LAN.
pub(crate) fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(Duration::from_secs(3))
        .no_proxy()
        .build()?)
}

/// `scheme://host[:port]` of an absolute URL.
pub(crate) fn origin(url: &str) -> Option<&str> {
    let sep = url.find("://")?;
    let after = sep + 3;
    let end = url[after..].find(['/', '?', '#']).map(|i| after + i).unwrap_or(url.len());
    (end > after).then(|| &url[..end])
}

/// Host part of an absolute URL, without userinfo, port or IPv6 brackets.
pub(crate) fn host(url: &str) -> Option<&str> {
    let authority = &origin(url)?[url.find("://")? + 3..];
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(v6) = authority.strip_prefix('[') {
        return v6.split(']').next();
    }
    authority.split(':').next().filter(|h| !h.is_empty())
}

/// Resolve `reference` against `base` like a browser would (RFC 3986 section 5, without
/// dot-segment removal). UPnP says relative URLs in a description resolve against
/// `URLBase`, or the description URL when there is none.
pub(crate) fn resolve(base: &str, reference: &str) -> String {
    let r = reference.trim();
    if r.contains("://") {
        return r.to_string();
    }
    let Some(origin) = origin(base) else { return r.to_string() };
    if let Some(net) = r.strip_prefix("//") {
        let scheme = &base[..base.find("://").unwrap_or(0)];
        return format!("{scheme}://{net}");
    }
    if r.starts_with('/') {
        return format!("{origin}{r}");
    }
    // Directory of the base path ("" or "/a/b/" style), dropping query and fragment.
    let path = &base[origin.len()..];
    let path = &path[..path.find(['?', '#']).unwrap_or(path.len())];
    let dir = path.rfind('/').map(|i| &path[..=i]).unwrap_or("/");
    let r = r.strip_prefix("./").unwrap_or(r);
    format!("{origin}{dir}{r}")
}

/// Percent-encode everything except RFC 3986 unreserved characters (query-value safe).
pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3 / 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Lowercase file extension of a URL's last path segment, ignoring query and fragment.
pub(crate) fn extension(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let path = path.split_once("://").map(|(_, rest)| rest.find('/').map(|i| &rest[i..]).unwrap_or("")).unwrap_or(path);
    let segment = path.rsplit('/').next()?;
    let (stem, ext) = segment.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

/// Best-effort MIME type from a URL's extension (debrid links usually keep the file name).
pub(crate) fn guess_mime(url: &str) -> Option<&'static str> {
    Some(match extension(url)?.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "ts" | "m2ts" | "mts" => "video/mp2t",
        "mpg" | "mpeg" => "video/mpeg",
        "wmv" => "video/x-ms-wmv",
        "m3u8" => "application/vnd.apple.mpegurl",
        "mpd" => "application/dash+xml",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_and_hosts() {
        assert_eq!(origin("http://192.168.1.20:9197/dmr"), Some("http://192.168.1.20:9197"));
        assert_eq!(origin("http://192.168.1.20:8060"), Some("http://192.168.1.20:8060"));
        assert_eq!(origin("http://h?x=1"), Some("http://h"));
        assert_eq!(origin("nope"), None);
        assert_eq!(host("http://192.168.1.20:9197/dmr"), Some("192.168.1.20"));
        assert_eq!(host("http://user@tv.local/x"), Some("tv.local"));
        assert_eq!(host("http://[fe80::1]:8060/"), Some("fe80::1"));
    }

    #[test]
    fn resolves_absolute_and_relative_references() {
        let base = "http://10.0.0.5:1400/xml/device_description.xml";
        assert_eq!(resolve(base, "http://10.0.0.6/ctl"), "http://10.0.0.6/ctl");
        assert_eq!(resolve(base, "//10.0.0.7:80/ctl"), "http://10.0.0.7:80/ctl");
        let abs_path = "http://10.0.0.5:1400/MediaRenderer/AVTransport/Control";
        assert_eq!(resolve(base, "/MediaRenderer/AVTransport/Control"), abs_path);
        assert_eq!(resolve(base, "AVTransport/Control"), "http://10.0.0.5:1400/xml/AVTransport/Control");
        assert_eq!(resolve("http://10.0.0.5:1400", "ctl"), "http://10.0.0.5:1400/ctl");
        assert_eq!(resolve("http://10.0.0.5:1400/", "./ctl"), "http://10.0.0.5:1400/ctl");
        assert_eq!(resolve("http://10.0.0.5/dmr?x=/y", "ctl"), "http://10.0.0.5/ctl");
    }

    #[test]
    fn percent_encodes_query_values() {
        assert_eq!(percent_encode("https://x.io/a b.mkv?t=1&u=2"), "https%3A%2F%2Fx.io%2Fa%20b.mkv%3Ft%3D1%26u%3D2");
        assert_eq!(percent_encode("Amélie"), "Am%C3%A9lie");
        assert_eq!(percent_encode("safe-._~"), "safe-._~");
    }

    #[test]
    fn guesses_mime_from_extension() {
        assert_eq!(guess_mime("https://dl.example/d/ABC/Movie.2020.1080p.MKV?token=1"), Some("video/x-matroska"));
        assert_eq!(guess_mime("https://dl.example/v/master.m3u8"), Some("application/vnd.apple.mpegurl"));
        assert_eq!(guess_mime("https://dl.example/d/ABC"), None);
        assert_eq!(guess_mime("https://dl.example.com"), None);
        assert_eq!(extension("https://x/.hidden"), None);
    }
}
