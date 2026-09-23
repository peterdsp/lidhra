//! Small helpers shared by the JSON-navigated adapters (no extra deps).

use crate::error::{DebridError, Result};
use crate::model::Credential;
use serde_json::Value;

/// String from a JSON string or number; empty for anything else.
pub(crate) fn jstr(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Number from a JSON number or numeric string (APIs are inconsistent about this).
pub(crate) fn jnum(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Bool from a JSON bool, 0/1 number, or "true"/"1" string.
pub(crate) fn jbool(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
        Some(Value::String(s)) => matches!(s.as_str(), "true" | "1" | "yes"),
        _ => false,
    }
}

/// The raw secret out of a [`Credential`] (API key or OAuth access token).
pub(crate) fn secret(cred: Credential) -> String {
    match cred {
        Credential::ApiKey(k) => k,
        Credential::OAuth { access, .. } => access,
    }
}

/// Map an HTTP status to a typed error; `Ok` for 2xx. Reads the body for context.
pub(crate) async fn check_status(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    match status.as_u16() {
        401 | 403 => Err(DebridError::Auth),
        429 => Err(DebridError::RateLimited),
        _ => {
            let body = resp.text().await.unwrap_or_default();
            Err(DebridError::Provider(format!("{status}: {body}")))
        }
    }
}

/// Last path segment of a URL, without query string. Falls back to "download".
pub(crate) fn filename_from_url(url: &str) -> String {
    let no_query = url.split('?').next().unwrap_or("");
    // Drop scheme + host so a bare origin yields the fallback, not the hostname.
    let path = match no_query.split_once("://") {
        Some((_, rest)) => rest.find('/').map(|i| &rest[i..]).unwrap_or(""),
        None => no_query,
    };
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("download")
        .to_string()
}

/// Parse a size that may be a byte count or a human string like `"1.50 GB"`.
pub(crate) fn parse_size(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
        Some(Value::String(s)) => parse_human_size(s),
        _ => 0,
    }
}

/// Unix seconds for a `YYYY-MM-DD` date at 00:00 UTC (some APIs report expiry as a date).
pub(crate) fn parse_ymd(s: &str) -> Option<i64> {
    let mut it = s.trim().splitn(3, '-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.get(..2)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm), epoch 1970-01-01.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp as i64 + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400)
}

/// Current Unix time in seconds.
pub(crate) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn parse_human_size(s: &str) -> u64 {
    let s = s.trim();
    if let Ok(n) = s.parse::<u64>() {
        return n;
    }
    let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let num: f64 = s[..split].trim().parse().unwrap_or(0.0);
    let unit = s[split..].trim().to_ascii_uppercase();
    let mult: f64 = match unit.as_str() {
        "" | "B" => 1.0,
        "K" | "KB" | "KIB" => 1024.0,
        "M" | "MB" | "MIB" => 1024.0 * 1024.0,
        "G" | "GB" | "GIB" => 1024.0 * 1024.0 * 1024.0,
        "T" | "TB" | "TIB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    (num * mult).round().max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size(Some(&json!(1234))), 1234);
        assert_eq!(parse_size(Some(&json!("1234"))), 1234);
        assert_eq!(parse_size(Some(&json!("1.50 GB"))), 1_610_612_736);
        assert_eq!(parse_size(Some(&json!("700MB"))), 734_003_200);
        assert_eq!(parse_size(Some(&json!("junk"))), 0);
        assert_eq!(parse_size(None), 0);
    }

    #[test]
    fn filename_from_urls() {
        assert_eq!(filename_from_url("https://x/a/b/file.mkv?token=1"), "file.mkv");
        assert_eq!(filename_from_url("https://x/"), "download");
        assert_eq!(filename_from_url(""), "download");
    }

    #[test]
    fn parses_ymd_dates() {
        assert_eq!(parse_ymd("1970-01-01"), Some(0));
        assert_eq!(parse_ymd("2000-03-01"), Some(951_868_800));
        assert_eq!(parse_ymd("2026-06-15"), Some(1_781_481_600));
        assert_eq!(parse_ymd("2026-06-15T10:00:00Z"), Some(1_781_481_600));
        assert_eq!(parse_ymd("nope"), None);
        assert_eq!(parse_ymd("2026-13-01"), None);
    }

    #[test]
    fn loose_json_readers() {
        assert_eq!(jnum(Some(&json!("42.5"))), 42.5);
        assert_eq!(jnum(Some(&json!(7))), 7.0);
        assert!(jbool(Some(&json!(1))));
        assert!(jbool(Some(&json!("true"))));
        assert!(!jbool(Some(&json!("no"))));
        assert_eq!(jstr(Some(&json!(12))), "12");
    }
}
