//! Offcloud adapter. Docs: <https://github.com/Offcloud/offcloud-api>.
//! Base `https://offcloud.com`, auth is the account API key as a `key` query
//! param. Responses are bare JSON objects; errors carry an `error` string
//! (`NOAUTH`, …) or a `not_available` message.
//!
//! Offcloud model: `POST /api/cloud` starts a cloud download identified by a
//! `requestId`; `cloud/status` reports progress; a finished request is either a
//! single file at `https://{server}.offcloud.com/cloud/download/{requestId}/{fileName}`
//! or a directory whose files come from `cloud/explore/{requestId}`. Those URLs are
//! already direct, so `unrestrict` passes them through. JSON-navigated; verify
//! field paths against a live account.

use super::util::{check_status, filename_from_url, jbool, jnum, jstr, parse_ymd, secret};
use crate::error::{DebridError, Result};
use crate::model::*;
use crate::provider::DebridProvider;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::RwLock;

const BASE: &str = "https://offcloud.com";

pub struct Offcloud {
    http: reqwest::Client,
    key: RwLock<String>,
}

impl Offcloud {
    pub fn new(key: impl Into<String>) -> Self {
        Offcloud { http: reqwest::Client::new(), key: RwLock::new(key.into()) }
    }

    fn key(&self) -> String {
        self.key.read().expect("key lock").clone()
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value> {
        let resp = req.query(&[("key", self.key())]).send().await?;
        let v: Value = check_status(resp).await?.json().await?;
        Self::unwrap(v)
    }

    async fn get(&self, path: &str) -> Result<Value> {
        self.send(self.http.get(format!("{BASE}{path}"))).await
    }

    async fn post_form(&self, path: &str, form: &[(&str, &str)]) -> Result<Value> {
        self.send(self.http.post(format!("{BASE}{path}")).form(form)).await
    }

    async fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        self.send(self.http.post(format!("{BASE}{path}")).json(body)).await
    }

    fn unwrap(v: Value) -> Result<Value> {
        let err = jstr(v.get("error"));
        if !err.is_empty() {
            return Err(oc_error(&err));
        }
        if let Some(na) = v.get("not_available").and_then(Value::as_str) {
            return Err(DebridError::Provider(format!("not available: {na}")));
        }
        Ok(v)
    }

    /// Current state of one request (`cloud/status` wraps it in `status` on some paths).
    async fn status(&self, id: &str) -> Result<Value> {
        let v = self.post_form("/api/cloud/status", &[("requestId", id)]).await?;
        Ok(match v.get("status") {
            Some(inner) if inner.is_object() => inner.clone(),
            _ => v,
        })
    }

    /// Direct links for a finished request: explore a directory, or build the
    /// single-file URL from `server` + `fileName`.
    async fn links_for(&self, s: &Value) -> Vec<RestrictedLink> {
        let id = jstr(s.get("requestId"));
        if jbool(s.get("isDirectory")) {
            if let Ok(v) = self.get(&format!("/api/cloud/explore/{id}")).await {
                if let Some(arr) = v.as_array() {
                    return arr
                        .iter()
                        .map(|e| if e.is_string() { jstr(Some(e)) } else { jstr(e.get("url")) })
                        .filter(|u| !u.is_empty())
                        .map(RestrictedLink)
                        .collect();
                }
            }
        }
        match single_file_url(s) {
            Some(u) => vec![RestrictedLink(u)],
            None => Vec::new(),
        }
    }

    async fn to_transfer(&self, s: &Value) -> RemoteTransfer {
        let status = oc_status(&jstr(s.get("status")));
        let links = if status == TransferStatus::Ready { self.links_for(s).await } else { Vec::new() };
        RemoteTransfer {
            id: TransferId(jstr(s.get("requestId"))),
            name: jstr(s.get("fileName")),
            status,
            progress: oc_progress(s, status),
            links,
        }
    }
}

fn oc_error(err: &str) -> DebridError {
    let e = err.to_ascii_lowercase();
    if e.contains("noauth") || e.contains("unauthorized") || e.contains("not premium") {
        DebridError::Auth
    } else {
        DebridError::Provider(err.to_string())
    }
}

fn oc_status(s: &str) -> TransferStatus {
    match s {
        "downloaded" => TransferStatus::Ready,
        "downloading" => TransferStatus::Downloading,
        "created" | "queued" => TransferStatus::Queued,
        "error" | "canceled" | "cancelled" => TransferStatus::Error,
        _ => TransferStatus::Queued,
    }
}

/// `amount` is bytes so far, `fileSize` the total; history rows carry no `amount`.
fn oc_progress(s: &Value, status: TransferStatus) -> f32 {
    if status == TransferStatus::Ready {
        return 1.0;
    }
    let done = jnum(s.get("amount"));
    let total = jnum(s.get("fileSize"));
    if total > 0.0 { (done / total).clamp(0.0, 1.0) as f32 } else { 0.0 }
}

fn single_file_url(s: &Value) -> Option<String> {
    let server = jstr(s.get("server"));
    let id = jstr(s.get("requestId"));
    let name = jstr(s.get("fileName"));
    if server.is_empty() || id.is_empty() || name.is_empty() {
        return None;
    }
    Some(format!("https://{server}.offcloud.com/cloud/download/{id}/{name}"))
}

#[async_trait]
impl DebridProvider for Offcloud {
    fn id(&self) -> ProviderId {
        ProviderId::Offcloud
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { magnet: true, torrent_file: false, batch_cache_check: true, streaming_link: false, folders: true }
    }

    async fn authenticate(&self, cred: Credential) -> Result<()> {
        *self.key.write().expect("key lock") = secret(cred);
        self.account().await.map(|_| ())
    }

    async fn account(&self) -> Result<AccountInfo> {
        let v = self.get("/api/account/info").await?;
        let username = {
            let e = jstr(v.get("email"));
            if e.is_empty() { jstr(v.get("user_id")) } else { e }
        };
        Ok(AccountInfo {
            username,
            premium: jbool(v.get("is_premium")),
            expires_at: parse_ymd(&jstr(v.get("expiration_date"))),
            traffic_left: None,
        })
    }

    async fn check_cache(&self, hashes: &[InfoHash]) -> Result<Vec<CacheStatus>> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let body = serde_json::json!({ "hashes": hashes.iter().map(|h| h.as_hex()).collect::<Vec<_>>() });
        let v = self.post_json("/api/cache", &body).await?;
        let cached: Vec<String> = v
            .get("cachedItems")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|x| jstr(Some(x)).to_ascii_lowercase()).collect())
            .unwrap_or_default();
        Ok(hashes
            .iter()
            .map(|h| CacheStatus { hash: h.clone(), cached: cached.iter().any(|c| c == h.as_hex()) })
            .collect())
    }

    async fn add_magnet(&self, magnet: &Magnet) -> Result<RemoteTransfer> {
        let v = self.post_form("/api/cloud", &[("url", &magnet.uri)]).await?;
        let id = jstr(v.get("requestId"));
        if id.is_empty() {
            return Err(DebridError::Provider("Offcloud: no requestId returned".into()));
        }
        self.transfer(&TransferId(id)).await
    }

    async fn add_torrent(&self, _torrent: &[u8]) -> Result<RemoteTransfer> {
        Err(DebridError::Provider("Offcloud: use add_magnet (file upload not implemented)".into()))
    }

    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>> {
        let v = self.get("/api/cloud/history").await?;
        let rows = match v.get("history") {
            Some(h) => h.as_array().cloned().unwrap_or_default(),
            None => v.as_array().cloned().unwrap_or_default(),
        };
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            out.push(self.to_transfer(r).await);
        }
        Ok(out)
    }

    async fn transfer(&self, id: &TransferId) -> Result<RemoteTransfer> {
        let mut s = self.status(&id.0).await?;
        if jstr(s.get("requestId")).is_empty() {
            s["requestId"] = Value::String(id.0.clone());
        }
        Ok(self.to_transfer(&s).await)
    }

    async fn unrestrict(&self, link: &RestrictedLink) -> Result<DirectLink> {
        // Cloud download URLs are already direct.
        Ok(DirectLink { url: link.0.clone(), filename: filename_from_url(&link.0), size: 0, mime: None })
    }

    async fn delete(&self, id: &TransferId) -> Result<()> {
        let v = self.get(&format!("/cloud/remove/{}", id.0)).await?;
        if v.get("success").and_then(Value::as_bool) == Some(false) {
            return Err(DebridError::Provider("Offcloud: remove refused".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_status_and_progress() {
        assert_eq!(oc_status("downloaded"), TransferStatus::Ready);
        assert_eq!(oc_status("downloading"), TransferStatus::Downloading);
        assert_eq!(oc_status("created"), TransferStatus::Queued);
        assert_eq!(oc_status("canceled"), TransferStatus::Error);
        let s = json!({ "amount": 25, "fileSize": 100 });
        assert_eq!(oc_progress(&s, TransferStatus::Downloading), 0.25);
        assert_eq!(oc_progress(&s, TransferStatus::Ready), 1.0);
        assert_eq!(oc_progress(&json!({}), TransferStatus::Queued), 0.0);
    }

    #[test]
    fn builds_single_file_url() {
        let s = json!({ "server": "s7", "requestId": "abc123", "fileName": "ubuntu.iso" });
        assert_eq!(single_file_url(&s).as_deref(), Some("https://s7.offcloud.com/cloud/download/abc123/ubuntu.iso"));
        assert!(single_file_url(&json!({ "requestId": "abc123" })).is_none());
    }

    #[test]
    fn maps_errors() {
        assert!(matches!(oc_error("NOAUTH"), DebridError::Auth));
        assert!(matches!(oc_error("Unauthorized"), DebridError::Auth));
        assert!(matches!(oc_error("limit reached"), DebridError::Provider(_)));
        assert!(Offcloud::unwrap(json!({ "not_available": "torrent" })).is_err());
        assert!(Offcloud::unwrap(json!({ "requestId": "x" })).is_ok());
    }
}
