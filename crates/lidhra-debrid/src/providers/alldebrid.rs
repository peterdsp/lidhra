//! AllDebrid adapter - API v4. Docs: <https://docs.alldebrid.com/>.
//! Auth is an API key passed as a query param, alongside a required `agent` name.
//! Envelope: `{ "status": "success", "data": {...} }` or `{ "status": "error", "error": {...} }`.
//!
//! JSON is navigated via `serde_json::Value` so small upstream shape changes don't
//! break compilation. Verify field paths against a live account before shipping.

use crate::error::{DebridError, Result};
use crate::model::*;
use crate::provider::DebridProvider;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::RwLock;

const BASE: &str = "https://api.alldebrid.com/v4";
const AGENT: &str = "lidhra";

pub struct AllDebrid {
    http: reqwest::Client,
    apikey: RwLock<String>,
}

impl AllDebrid {
    pub fn new(apikey: impl Into<String>) -> Self {
        AllDebrid { http: reqwest::Client::new(), apikey: RwLock::new(apikey.into()) }
    }

    fn key(&self) -> String {
        self.apikey.read().expect("key lock").clone()
    }

    async fn call(&self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        let key = self.key();
        let mut q: Vec<(&str, &str)> = vec![("agent", AGENT), ("apikey", &key)];
        q.extend_from_slice(params);
        let v: Value = self.http.get(format!("{BASE}{path}")).query(&q).send().await?.json().await?;
        if v.get("status").and_then(Value::as_str) == Some("success") {
            Ok(v.get("data").cloned().unwrap_or(Value::Null))
        } else {
            let code = v.pointer("/error/code").and_then(Value::as_str).unwrap_or("");
            let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("error");
            if code.contains("AUTH") {
                Err(DebridError::Auth)
            } else {
                Err(DebridError::Provider(format!("{code}: {msg}")))
            }
        }
    }
}

fn jstr(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn ad_status(code: i64) -> TransferStatus {
    match code {
        0 => TransferStatus::Queued,       // in queue
        1..=3 => TransferStatus::Downloading, // downloading / compressing / uploading
        4 => TransferStatus::Ready,        // ready
        _ => TransferStatus::Error,
    }
}

fn ad_transfer(m: &Value) -> RemoteTransfer {
    let code = m.get("statusCode").and_then(Value::as_i64).unwrap_or(-1);
    let downloaded = m.get("downloaded").and_then(Value::as_f64).unwrap_or(0.0);
    let size = m.get("size").and_then(Value::as_f64).unwrap_or(0.0);
    let progress = if size > 0.0 { (downloaded / size) as f32 } else { 0.0 };
    let links = m
        .get("links")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|l| l.get("link").and_then(Value::as_str)).map(|s| RestrictedLink(s.to_string())).collect())
        .unwrap_or_default();
    RemoteTransfer {
        id: TransferId(jstr(m.get("id"))),
        name: jstr(m.get("filename")),
        status: ad_status(code),
        progress: progress.clamp(0.0, 1.0),
        links,
    }
}

#[async_trait]
impl DebridProvider for AllDebrid {
    fn id(&self) -> ProviderId {
        ProviderId::AllDebrid
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { magnet: true, torrent_file: false, batch_cache_check: false, streaming_link: true, folders: true }
    }

    async fn authenticate(&self, cred: Credential) -> Result<()> {
        let key = match cred {
            Credential::ApiKey(k) => k,
            Credential::OAuth { access, .. } => access,
        };
        *self.apikey.write().expect("key lock") = key;
        self.account().await.map(|_| ())
    }

    async fn account(&self) -> Result<AccountInfo> {
        let data = self.call("/user", &[]).await?;
        let u = data.get("user").unwrap_or(&data);
        Ok(AccountInfo {
            username: jstr(u.get("username")),
            premium: u.get("isPremium").and_then(Value::as_bool).unwrap_or(false),
            expires_at: u.get("premiumUntil").and_then(Value::as_i64),
            traffic_left: None,
        })
    }

    async fn check_cache(&self, hashes: &[InfoHash]) -> Result<Vec<CacheStatus>> {
        // No stable batch cache endpoint; report unknown → callers add + poll.
        Ok(hashes.iter().cloned().map(|hash| CacheStatus { hash, cached: false }).collect())
    }

    async fn add_magnet(&self, magnet: &Magnet) -> Result<RemoteTransfer> {
        let data = self.call("/magnet/upload", &[("magnets[]", &magnet.uri)]).await?;
        let first = data.pointer("/magnets/0").ok_or_else(|| DebridError::Provider("no magnet in response".into()))?;
        if let Some(err) = first.pointer("/error/message").and_then(Value::as_str) {
            return Err(DebridError::Provider(err.to_string()));
        }
        let id = jstr(first.get("id"));
        // Fetch full status so we get files/links/progress uniformly.
        self.transfer(&TransferId(id)).await
    }

    async fn add_torrent(&self, _torrent: &[u8]) -> Result<RemoteTransfer> {
        Err(DebridError::Provider("AllDebrid: use add_magnet (file upload not implemented)".into()))
    }

    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>> {
        let data = self.call("/magnet/status", &[]).await?;
        let arr = data.get("magnets").and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(arr.iter().map(ad_transfer).collect())
    }

    async fn transfer(&self, id: &TransferId) -> Result<RemoteTransfer> {
        let data = self.call("/magnet/status", &[("id", &id.0)]).await?;
        let m = data.get("magnets").unwrap_or(&data);
        Ok(ad_transfer(m))
    }

    async fn unrestrict(&self, link: &RestrictedLink) -> Result<DirectLink> {
        let data = self.call("/link/unlock", &[("link", &link.0)]).await?;
        Ok(DirectLink {
            url: jstr(data.get("link")),
            filename: jstr(data.get("filename")),
            size: data.get("filesize").and_then(Value::as_u64).unwrap_or(0),
            mime: None,
        })
    }

    async fn delete(&self, id: &TransferId) -> Result<()> {
        self.call("/magnet/delete", &[("id", &id.0)]).await.map(|_| ())
    }
}
