use crate::error::{DebridError, Result};
use serde::{Deserialize, Serialize};

/// Which service an adapter speaks to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderId {
    RealDebrid,
    AllDebrid,
    TorBox,
    Premiumize,
    DebridLink,
    Offcloud,
    MegaDebrid,
    Deepbrid,
    HighWay,
}

impl ProviderId {
    pub fn label(self) -> &'static str {
        match self {
            ProviderId::RealDebrid => "Real-Debrid",
            ProviderId::AllDebrid => "AllDebrid",
            ProviderId::TorBox => "TorBox",
            ProviderId::Premiumize => "Premiumize",
            ProviderId::DebridLink => "Debrid-Link",
            ProviderId::Offcloud => "Offcloud",
            ProviderId::MegaDebrid => "Mega-Debrid",
            ProviderId::Deepbrid => "Deepbrid",
            ProviderId::HighWay => "High-Way",
        }
    }
}

/// What a provider can do — used by the policy engine and UI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub magnet: bool,
    pub torrent_file: bool,
    /// Can check many info-hashes for cache status in one call.
    pub batch_cache_check: bool,
    pub streaming_link: bool,
    pub folders: bool,
}

/// Bring-Your-Own-Account only. Lidhra never ships or brokers accounts.
#[derive(Clone, Debug)]
pub enum Credential {
    ApiKey(String),
    /// e.g. Real-Debrid OAuth2 device-code flow.
    OAuth { access: String, refresh: String },
}

#[derive(Clone, Debug)]
pub struct Account {
    pub provider: ProviderId,
    pub info: AccountInfo,
}

#[derive(Clone, Debug, Default)]
pub struct AccountInfo {
    pub username: String,
    pub premium: bool,
    /// Unix seconds, if the provider reports an expiry.
    pub expires_at: Option<i64>,
    /// Remaining traffic in bytes, if metered.
    pub traffic_left: Option<u64>,
}

/// A 20-byte BitTorrent info-hash, stored as lowercase hex for API friendliness.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InfoHash(String);

impl InfoHash {
    pub fn as_hex(&self) -> &str {
        &self.0
    }

    /// Accepts a 40-char hex hash or a 32-char base32 hash (RFC 4648).
    pub fn parse(raw: &str) -> Result<Self> {
        let s = raw.trim();
        match s.len() {
            40 if s.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(InfoHash(s.to_ascii_lowercase())),
            32 => {
                let bytes = base32_decode(s)
                    .ok_or_else(|| DebridError::BadMagnet(format!("bad base32 hash: {s}")))?;
                Ok(InfoHash(hex_encode(&bytes)))
            }
            _ => Err(DebridError::BadMagnet(format!("not a valid info-hash: {s}"))),
        }
    }
}

impl std::fmt::Display for InfoHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A parsed magnet link.
#[derive(Clone, Debug)]
pub struct Magnet {
    pub hash: InfoHash,
    pub uri: String,
    pub display_name: Option<String>,
}

impl Magnet {
    /// Parse a `magnet:?xt=urn:btih:<hash>&dn=<name>` URI.
    pub fn parse(uri: &str) -> Result<Self> {
        if !uri.starts_with("magnet:?") {
            return Err(DebridError::BadMagnet("not a magnet URI".into()));
        }
        let query = &uri["magnet:?".len()..];
        let mut hash = None;
        let mut name = None;
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            match k {
                "xt" => {
                    if let Some(h) = v.strip_prefix("urn:btih:") {
                        hash = Some(InfoHash::parse(h)?);
                    }
                }
                "dn" => name = Some(percent_decode(v)),
                _ => {}
            }
        }
        let hash = hash.ok_or_else(|| DebridError::BadMagnet("no btih in magnet".into()))?;
        Ok(Magnet { hash, uri: uri.to_string(), display_name: name })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheStatus {
    pub hash: InfoHash,
    /// True if the provider already has it ("instant").
    pub cached: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferStatus {
    Queued,
    Downloading,
    Ready,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferId(pub String);

/// A transfer living on the provider's side.
#[derive(Clone, Debug)]
pub struct RemoteTransfer {
    pub id: TransferId,
    pub name: String,
    pub status: TransferStatus,
    /// 0.0..=1.0
    pub progress: f32,
    /// Restricted links to resolve via [`crate::DebridProvider::unrestrict`].
    pub links: Vec<RestrictedLink>,
}

#[derive(Clone, Debug)]
pub struct RestrictedLink(pub String);

/// The end goal: a plain TLS HTTPS URL for Lidhra's download engine.
#[derive(Clone, Debug)]
pub struct DirectLink {
    pub url: String,
    pub filename: String,
    pub size: u64,
    pub mime: Option<String>,
}

// ---------- small self-contained helpers (no extra deps) ----------

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// RFC 4648 base32 decode (uppercase, no padding needed for 32-char hashes).
fn base32_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut buffer: u64 = 0;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for c in input.trim_end_matches('=').bytes() {
        let up = c.to_ascii_uppercase();
        let val = ALPHABET.iter().position(|&a| a == up)? as u64;
        buffer = (buffer << 5) | val;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// Minimal percent-decoder for display names (`%20`, `+`).
fn percent_decode(s: &str) -> String {
    let bytes = s.replace('+', " ");
    let raw = bytes.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let Ok(b) = u8::from_str_radix(&bytes[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_magnet() {
        let m = Magnet::parse(
            "magnet:?xt=urn:btih:2c6b6858d61da9543d4231a71db4b1c9264b0685&dn=ubuntu+24.04.iso",
        )
        .unwrap();
        assert_eq!(m.hash.as_hex(), "2c6b6858d61da9543d4231a71db4b1c9264b0685");
        assert_eq!(m.display_name.as_deref(), Some("ubuntu 24.04.iso"));
    }

    #[test]
    fn parses_base32_hash() {
        // 32-char base32 decodes to the same 20-byte hash.
        let h = InfoHash::parse("FRVWQWGWDWUVIPKCGGTXDNFRZETEWBUF").unwrap();
        assert_eq!(h.as_hex().len(), 40);
    }

    #[test]
    fn rejects_junk() {
        assert!(Magnet::parse("https://example.com").is_err());
        assert!(InfoHash::parse("nope").is_err());
    }
}
