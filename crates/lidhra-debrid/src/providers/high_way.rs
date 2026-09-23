//! High-Way adapter - the transfer API at `https://cloud.high-way.me/api/transfers.php`.
//! High-Way publishes no API reference page; this follows the client that ships in
//! torrentio (verified against production by High-Way staff) and the same Bearer
//! API token users create for rdt-client / JDownloader (Center → API token).
//!
//! Verified calls: cache check (`?check=<hashes>&service=torrent`), add
//! (`POST magnet=<info-hash>` → `{ state, id, service }`, idempotent per hash), and
//! status (`?id=&service=` → `{ state, files: [{ path, size, download_url }] }`).
//! Listing all transfers and deleting one are *not* documented; the calls below
//! follow the same shape and fail cleanly if the server disagrees. File
//! `download_url`s are already direct, so `unrestrict` passes them through.

use super::util::{check_status, filename_from_url, jnum, jstr, secret};
use crate::error::{DebridError, Result};
use crate::model::*;
use crate::provider::DebridProvider;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::RwLock;

const BASE: &str = "https://cloud.high-way.me/api/transfers.php";
const SERVICE: &str = "torrent";

pub struct HighWay {
    http: reqwest::Client,
    token: RwLock<String>,
}

impl HighWay {
    pub fn new(token: impl Into<String>) -> Self {
        HighWay { http: reqwest::Client::new(), token: RwLock::new(token.into()) }
    }

    fn token(&self) -> String {
        self.token.read().expect("token lock").clone()
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value> {
        let resp = req.bearer_auth(self.token()).send().await?;
        let v: Value = check_status(resp).await?.json().await?;
        Self::unwrap(v)
    }

    async fn get(&self, query: &[(&str, &str)]) -> Result<Value> {
        self.send(self.http.get(BASE).query(query)).await
    }

    async fn post_form(&self, form: &[(&str, &str)]) -> Result<Value> {
        self.send(self.http.post(BASE).form(form)).await
    }

    fn unwrap(v: Value) -> Result<Value> {
        let err = jstr(v.get("error"));
        if err.is_empty() {
            return Ok(v);
        }
        let lower = err.to_ascii_lowercase();
        if lower.contains("auth") || lower.contains("token") || lower.contains("log") {
            Err(DebridError::Auth)
        } else {
            let msg = jstr(v.get("message"));
            Err(DebridError::Provider(if msg.is_empty() { err } else { format!("{err}: {msg}") }))
        }
    }
}

fn hw_status(state: &str, files: usize) -> TransferStatus {
    let s = state.to_ascii_lowercase();
    if s == "completed" || s == "complete" || s == "finished" || s == "ready" {
        TransferStatus::Ready
    } else if s.contains("error") || s.contains("fail") || s == "cancelled" || s == "canceled" {
        TransferStatus::Error
    } else if s.contains("queue") || s.contains("pending") || s.contains("wait") {
        TransferStatus::Queued
    } else if s.is_empty() && files > 0 {
        TransferStatus::Ready
    } else {
        TransferStatus::Downloading
    }
}

fn hw_transfer(t: &Value) -> RemoteTransfer {
    let files: Vec<&Value> = t.get("files").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let status = hw_status(&jstr(t.get("state")), files.len());
    let raw = jnum(t.get("progress"));
    let progress = if status == TransferStatus::Ready {
        1.0
    } else if raw > 1.0 {
        (raw / 100.0).clamp(0.0, 1.0) as f32
    } else {
        raw.clamp(0.0, 1.0) as f32
    };
    let links = files
        .iter()
        .map(|f| jstr(f.get("download_url")))
        .filter(|u| !u.is_empty())
        .map(RestrictedLink)
        .collect();
    let name = {
        let n = jstr(t.get("name"));
        if !n.is_empty() {
            n
        } else {
            files.first().map(|f| jstr(f.get("path"))).unwrap_or_default()
        }
    };
    RemoteTransfer { id: TransferId(jstr(t.get("id"))), name, status, progress, links }
}

#[async_trait]
impl DebridProvider for HighWay {
    fn id(&self) -> ProviderId {
        ProviderId::HighWay
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { magnet: true, torrent_file: false, batch_cache_check: true, streaming_link: false, folders: true }
    }

    async fn authenticate(&self, cred: Credential) -> Result<()> {
        *self.token.write().expect("token lock") = secret(cred);
        self.account().await.map(|_| ())
    }

    async fn account(&self) -> Result<AccountInfo> {
        // The transfer API exposes no account call, so validate the token with a
        // cache probe (401/403 → Auth). Premium status is not reported here.
        self.get(&[("check", "0000000000000000000000000000000000000000"), ("service", SERVICE)]).await?;
        Ok(AccountInfo { username: String::new(), premium: true, expires_at: None, traffic_left: None })
    }

    async fn check_cache(&self, hashes: &[InfoHash]) -> Result<Vec<CacheStatus>> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let joined = hashes.iter().map(|h| h.as_hex()).collect::<Vec<_>>().join(",");
        let v = self.get(&[("check", &joined), ("service", SERVICE)]).await?;
        let results = v.get("results").cloned().unwrap_or(Value::Null);
        Ok(hashes
            .iter()
            .map(|h| {
                let e = results.get(h.as_hex());
                let cached = e
                    .map(|e| e.get("known").and_then(Value::as_bool).unwrap_or(false) && e.get("ready").and_then(Value::as_bool).unwrap_or(false))
                    .unwrap_or(false);
                CacheStatus { hash: h.clone(), cached }
            })
            .collect())
    }

    async fn add_magnet(&self, magnet: &Magnet) -> Result<RemoteTransfer> {
        // The verified client passes the bare info-hash as `magnet`.
        let v = self.post_form(&[("magnet", magnet.hash.as_hex())]).await?;
        let id = jstr(v.get("id"));
        if id.is_empty() {
            return Err(DebridError::Provider("High-Way: no transfer id returned".into()));
        }
        self.transfer(&TransferId(id)).await
    }

    async fn add_torrent(&self, _torrent: &[u8]) -> Result<RemoteTransfer> {
        Err(DebridError::Provider("High-Way: use add_magnet (file upload not implemented)".into()))
    }

    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>> {
        // Undocumented: assume the collection shape when no id is given.
        let v = self.get(&[("service", SERVICE)]).await?;
        let rows: Vec<Value> = match &v {
            Value::Array(a) => a.clone(),
            Value::Object(_) => ["transfers", "results", "data"]
                .iter()
                .find_map(|k| v.get(k).and_then(Value::as_array).cloned())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        Ok(rows.iter().filter(|r| !jstr(r.get("id")).is_empty()).map(hw_transfer).collect())
    }

    async fn transfer(&self, id: &TransferId) -> Result<RemoteTransfer> {
        let mut v = self.get(&[("id", &id.0), ("service", SERVICE)]).await?;
        if jstr(v.get("id")).is_empty() {
            v["id"] = Value::String(id.0.clone());
        }
        Ok(hw_transfer(&v))
    }

    async fn unrestrict(&self, link: &RestrictedLink) -> Result<DirectLink> {
        // `download_url`s are already direct.
        Ok(DirectLink { url: link.0.clone(), filename: filename_from_url(&link.0), size: 0, mime: None })
    }

    async fn delete(&self, id: &TransferId) -> Result<()> {
        // Undocumented: mirror the status call with the DELETE verb.
        self.send(self.http.delete(BASE).query(&[("id", id.0.as_str()), ("service", SERVICE)])).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_states() {
        assert_eq!(hw_status("completed", 0), TransferStatus::Ready);
        assert_eq!(hw_status("downloading", 0), TransferStatus::Downloading);
        assert_eq!(hw_status("queued", 0), TransferStatus::Queued);
        assert_eq!(hw_status("error", 0), TransferStatus::Error);
        assert_eq!(hw_status("", 2), TransferStatus::Ready);
        assert_eq!(hw_status("", 0), TransferStatus::Downloading);
    }

    #[test]
    fn maps_transfer_files() {
        let t = json!({ "id": 7, "state": "completed", "files": [
            { "path": "Show/ep1.mkv", "size": 10, "download_url": "https://dl.high-way.me/a/ep1.mkv" },
            { "path": "Show/ep2.mkv", "size": 10, "download_url": "https://dl.high-way.me/a/ep2.mkv" }
        ]});
        let r = hw_transfer(&t);
        assert_eq!(r.id.0, "7");
        assert_eq!(r.status, TransferStatus::Ready);
        assert_eq!(r.progress, 1.0);
        assert_eq!(r.links.len(), 2);
        assert_eq!(r.name, "Show/ep1.mkv");
        let d = hw_transfer(&json!({ "id": "8", "state": "downloading", "progress": 42 }));
        assert_eq!(d.progress, 0.42);
        let f = hw_transfer(&json!({ "id": "9", "state": "downloading", "progress": 0.5 }));
        assert_eq!(f.progress, 0.5);
    }

    #[test]
    fn maps_errors() {
        assert!(matches!(HighWay::unwrap(json!({ "error": "NotLoggedIn", "loggedin": false })), Err(DebridError::Auth)));
        assert!(matches!(HighWay::unwrap(json!({ "error": "quota", "message": "full" })), Err(DebridError::Provider(_))));
        assert!(HighWay::unwrap(json!({ "state": "completed" })).is_ok());
    }
}
