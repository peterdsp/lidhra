//! Debrid-Link adapter - API v2. Docs: <https://debrid-link.com/api_doc/v2/introduction>.
//! Base `https://debrid-link.com/api/v2`, auth is a Bearer API token
//! (debrid-link.com/webapp/apikey).
//! Envelope: `{ "success": true, "value": ... }` or
//! `{ "success": false, "error": "badToken", "error_description": "..." }`.
//!
//! Seedbox files already carry a direct `downloadUrl`, so `unrestrict` passes
//! seedbox links through; any other URL goes to `downloader/add` (hoster unlock).
//! JSON-navigated for resilience; verify field paths against a live account.

use super::util::{check_status, filename_from_url, jbool, jnum, jstr, now_unix, secret};
use crate::error::{DebridError, Result};
use crate::model::*;
use crate::provider::DebridProvider;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::RwLock;

const BASE: &str = "https://debrid-link.com/api/v2";

pub struct DebridLink {
    http: reqwest::Client,
    token: RwLock<String>,
}

impl DebridLink {
    pub fn new(token: impl Into<String>) -> Self {
        DebridLink { http: reqwest::Client::new(), token: RwLock::new(token.into()) }
    }

    fn token(&self) -> String {
        self.token.read().expect("token lock").clone()
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value> {
        let resp = req.bearer_auth(self.token()).send().await?;
        let v: Value = check_status(resp).await?.json().await?;
        Self::unwrap(v)
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        self.send(self.http.get(format!("{BASE}{path}")).query(query)).await
    }

    async fn post_form(&self, path: &str, form: &[(&str, &str)]) -> Result<Value> {
        self.send(self.http.post(format!("{BASE}{path}")).form(form)).await
    }

    fn unwrap(v: Value) -> Result<Value> {
        if v.get("success").and_then(Value::as_bool) == Some(true) {
            return Ok(v.get("value").cloned().unwrap_or(Value::Null));
        }
        let code = jstr(v.get("error"));
        let desc = jstr(v.get("error_description"));
        Err(dl_error(&code, &desc))
    }
}

fn dl_error(code: &str, desc: &str) -> DebridError {
    match code {
        "badToken" | "accountLocked" | "unauthorized" | "authorization" => DebridError::Auth,
        "floodDetected" => DebridError::RateLimited,
        _ if desc.is_empty() => DebridError::Provider(code.to_string()),
        _ => DebridError::Provider(format!("{code}: {desc}")),
    }
}

/// Seedbox torrent → uniform transfer. `downloadPercent` is 0-100; `wait` means
/// the torrent is parked until files are selected; `error` is non-zero on failure.
fn dl_status(t: &Value) -> TransferStatus {
    let percent = jnum(t.get("downloadPercent"));
    let failed = jnum(t.get("error")) != 0.0 || !jstr(t.get("errorString")).is_empty();
    if failed {
        TransferStatus::Error
    } else if percent >= 100.0 {
        TransferStatus::Ready
    } else if jbool(t.get("wait")) {
        TransferStatus::Queued
    } else {
        TransferStatus::Downloading
    }
}

fn dl_transfer(t: &Value) -> RemoteTransfer {
    let links = t
        .get("files")
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .map(|f| jstr(f.get("downloadUrl")))
                .filter(|u| !u.is_empty())
                .map(RestrictedLink)
                .collect()
        })
        .unwrap_or_default();
    RemoteTransfer {
        id: TransferId(jstr(t.get("id"))),
        name: jstr(t.get("name")),
        status: dl_status(t),
        progress: (jnum(t.get("downloadPercent")) / 100.0).clamp(0.0, 1.0) as f32,
        links,
    }
}

/// `value` may be a single torrent or a one-element list depending on the call.
fn first_object(v: Value) -> Value {
    match v {
        Value::Array(mut a) if !a.is_empty() => a.remove(0),
        other => other,
    }
}

#[async_trait]
impl DebridProvider for DebridLink {
    fn id(&self) -> ProviderId {
        ProviderId::DebridLink
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { magnet: true, torrent_file: false, batch_cache_check: true, streaming_link: true, folders: true }
    }

    async fn authenticate(&self, cred: Credential) -> Result<()> {
        *self.token.write().expect("token lock") = secret(cred);
        self.account().await.map(|_| ())
    }

    async fn account(&self) -> Result<AccountInfo> {
        let v = self.get("/account/infos", &[]).await?;
        let username = {
            let u = jstr(v.get("username"));
            if u.is_empty() { jstr(v.get("email")) } else { u }
        };
        let premium_left = jnum(v.get("premiumLeft")) as i64;
        let premium = premium_left > 0 || jnum(v.get("accountType")) > 0.0;
        Ok(AccountInfo {
            username,
            premium,
            expires_at: (premium_left > 0).then(|| now_unix() + premium_left),
            traffic_left: None,
        })
    }

    async fn check_cache(&self, hashes: &[InfoHash]) -> Result<Vec<CacheStatus>> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        // `seedbox/cached` is deprecated upstream; a failure means "unknown", not "no".
        let joined = hashes.iter().map(|h| h.as_hex()).collect::<Vec<_>>().join(",");
        let v = self.get("/seedbox/cached", &[("url", &joined)]).await.unwrap_or(Value::Null);
        Ok(hashes
            .iter()
            .map(|h| CacheStatus {
                hash: h.clone(),
                cached: v.get(h.as_hex()).map(|e| !e.is_null()).unwrap_or(false),
            })
            .collect())
    }

    async fn add_magnet(&self, magnet: &Magnet) -> Result<RemoteTransfer> {
        let v = self.post_form("/seedbox/add", &[("url", &magnet.uri), ("async", "true")]).await?;
        let id = jstr(first_object(v).get("id"));
        if id.is_empty() {
            return Err(DebridError::Provider("Debrid-Link: no torrent id returned".into()));
        }
        self.transfer(&TransferId(id)).await
    }

    async fn add_torrent(&self, _torrent: &[u8]) -> Result<RemoteTransfer> {
        Err(DebridError::Provider("Debrid-Link: use add_magnet (file upload not implemented)".into()))
    }

    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>> {
        let v = self.get("/seedbox/list", &[("perPage", "100")]).await?;
        Ok(v.as_array().map(|a| a.iter().map(dl_transfer).collect()).unwrap_or_default())
    }

    async fn transfer(&self, id: &TransferId) -> Result<RemoteTransfer> {
        let v = self.get("/seedbox/list", &[("ids", &id.0)]).await?;
        let t = first_object(v);
        if t.is_null() || jstr(t.get("id")).is_empty() {
            return Err(DebridError::Provider(format!("transfer {} not found", id.0)));
        }
        Ok(dl_transfer(&t))
    }

    async fn unrestrict(&self, link: &RestrictedLink) -> Result<DirectLink> {
        if is_seedbox_url(&link.0) {
            // Seedbox `downloadUrl`s are already direct.
            return Ok(DirectLink { url: link.0.clone(), filename: filename_from_url(&link.0), size: 0, mime: None });
        }
        let v = first_object(self.post_form("/downloader/add", &[("url", &link.0)]).await?);
        let url = jstr(v.get("downloadUrl"));
        if url.is_empty() {
            return Err(DebridError::Provider("Debrid-Link: no downloadUrl returned".into()));
        }
        let name = {
            let n = jstr(v.get("name"));
            if n.is_empty() { filename_from_url(&url) } else { n }
        };
        Ok(DirectLink { url, filename: name, size: jnum(v.get("size")) as u64, mime: None })
    }

    async fn delete(&self, id: &TransferId) -> Result<()> {
        self.send(self.http.delete(format!("{BASE}/seedbox/{}/remove", id.0))).await.map(|_| ())
    }
}

/// Seedbox file URLs live on debrid-link's own hosts (`*.debrid-link.com` / `.fr`).
fn is_seedbox_url(url: &str) -> bool {
    let host = url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("");
    host.contains("debrid-link.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_seedbox_status_and_links() {
        let t = json!({
            "id": "abc", "name": "ubuntu.iso", "downloadPercent": 100, "error": 0, "wait": false,
            "files": [{ "name": "ubuntu.iso", "downloadUrl": "https://s1.debrid-link.com/dl/x/ubuntu.iso", "size": 10 }]
        });
        let r = dl_transfer(&t);
        assert_eq!(r.status, TransferStatus::Ready);
        assert_eq!(r.progress, 1.0);
        assert_eq!(r.links.len(), 1);

        assert_eq!(dl_status(&json!({ "downloadPercent": 42, "wait": false })), TransferStatus::Downloading);
        assert_eq!(dl_status(&json!({ "downloadPercent": 0, "wait": true })), TransferStatus::Queued);
        assert_eq!(dl_status(&json!({ "downloadPercent": 50, "error": 3, "errorString": "dead" })), TransferStatus::Error);
    }

    #[test]
    fn maps_errors() {
        assert!(matches!(dl_error("badToken", ""), DebridError::Auth));
        assert!(matches!(dl_error("floodDetected", ""), DebridError::RateLimited));
        assert!(matches!(dl_error("maxTorrent", "limit"), DebridError::Provider(_)));
        assert!(matches!(DebridLink::unwrap(json!({ "success": true, "value": [1] })), Ok(Value::Array(_))));
        assert!(DebridLink::unwrap(json!({ "success": false, "error": "badToken" })).is_err());
    }

    #[test]
    fn recognises_seedbox_hosts() {
        assert!(is_seedbox_url("https://s12.debrid-link.com/dl/abc/file.mkv"));
        assert!(is_seedbox_url("https://cdn.debrid-link.fr/x"));
        assert!(!is_seedbox_url("https://rapidgator.net/file/abc"));
    }
}
