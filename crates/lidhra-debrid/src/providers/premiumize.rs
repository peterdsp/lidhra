//! Premiumize adapter - API at <https://www.premiumize.me/api>.
//! Auth is an API key passed as a query param. Envelope: `{ "status": "success", ... }`.
//!
//! Premiumize model: `transfer/create` starts a cloud task; when finished it
//! exposes a folder whose files carry already-direct HTTPS links. So `unrestrict`
//! is a near no-op (the link is direct). JSON-navigated for resilience; verify
//! field paths against a live account.

use crate::error::{DebridError, Result};
use crate::model::*;
use crate::provider::DebridProvider;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::RwLock;

const BASE: &str = "https://www.premiumize.me/api";

pub struct Premiumize {
    http: reqwest::Client,
    apikey: RwLock<String>,
}

impl Premiumize {
    pub fn new(apikey: impl Into<String>) -> Self {
        Premiumize { http: reqwest::Client::new(), apikey: RwLock::new(apikey.into()) }
    }
    fn key(&self) -> String {
        self.apikey.read().expect("key lock").clone()
    }

    async fn get(&self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        let key = self.key();
        let mut q: Vec<(&str, &str)> = vec![("apikey", &key)];
        q.extend_from_slice(params);
        let v: Value = self.http.get(format!("{BASE}{path}")).query(&q).send().await?.json().await?;
        Self::unwrap(v)
    }

    async fn post(&self, path: &str, form: &[(&str, &str)]) -> Result<Value> {
        let key = self.key();
        let v: Value = self
            .http
            .post(format!("{BASE}{path}"))
            .query(&[("apikey", key.as_str())])
            .form(form)
            .send()
            .await?
            .json()
            .await?;
        Self::unwrap(v)
    }

    fn unwrap(v: Value) -> Result<Value> {
        if v.get("status").and_then(Value::as_str) == Some("success") {
            Ok(v)
        } else {
            let msg = v.get("message").and_then(Value::as_str).unwrap_or("error");
            if msg.to_ascii_lowercase().contains("api key") {
                Err(DebridError::Auth)
            } else {
                Err(DebridError::Provider(msg.to_string()))
            }
        }
    }

    /// Collect already-direct file links from a finished transfer's folder.
    async fn folder_links(&self, folder_id: &str) -> Vec<RestrictedLink> {
        match self.get("/folder/list", &[("id", folder_id)]).await {
            Ok(v) => v
                .get("content")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter(|it| it.get("type").and_then(Value::as_str) == Some("file"))
                        .filter_map(|it| it.get("link").and_then(Value::as_str))
                        .map(|l| RestrictedLink(l.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            Err(_) => Vec::new(),
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

fn pm_status(s: &str) -> TransferStatus {
    match s {
        "finished" | "seeding" => TransferStatus::Ready,
        "running" | "downloading" => TransferStatus::Downloading,
        "waiting" | "queued" => TransferStatus::Queued,
        _ => TransferStatus::Error,
    }
}

#[async_trait]
impl DebridProvider for Premiumize {
    fn id(&self) -> ProviderId {
        ProviderId::Premiumize
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { magnet: true, torrent_file: false, batch_cache_check: true, streaming_link: true, folders: true }
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
        let v = self.get("/account/info", &[]).await?;
        let until = v.get("premium_until").and_then(Value::as_i64);
        Ok(AccountInfo {
            username: jstr(v.get("customer_id")),
            premium: until.map(|u| u > 0).unwrap_or(false),
            expires_at: until,
            traffic_left: None,
        })
    }

    async fn check_cache(&self, hashes: &[InfoHash]) -> Result<Vec<CacheStatus>> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let form: Vec<(&str, &str)> = hashes.iter().map(|h| ("items[]", h.as_hex())).collect();
        let v = self.post("/cache/check", &form).await?;
        let flags = v.get("response").and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(hashes
            .iter()
            .enumerate()
            .map(|(i, h)| CacheStatus {
                hash: h.clone(),
                cached: flags.get(i).and_then(Value::as_bool).unwrap_or(false),
            })
            .collect())
    }

    async fn add_magnet(&self, magnet: &Magnet) -> Result<RemoteTransfer> {
        let v = self.post("/transfer/create", &[("src", &magnet.uri)]).await?;
        let id = jstr(v.get("id"));
        if id.is_empty() {
            return Err(DebridError::Provider("Premiumize: no transfer id".into()));
        }
        self.transfer(&TransferId(id)).await
    }

    async fn add_torrent(&self, _torrent: &[u8]) -> Result<RemoteTransfer> {
        Err(DebridError::Provider("Premiumize: use add_magnet".into()))
    }

    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>> {
        let v = self.get("/transfer/list", &[]).await?;
        let arr = v.get("transfers").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut out = Vec::with_capacity(arr.len());
        for t in &arr {
            out.push(self.to_transfer(t).await);
        }
        Ok(out)
    }

    async fn transfer(&self, id: &TransferId) -> Result<RemoteTransfer> {
        let v = self.get("/transfer/list", &[]).await?;
        let arr = v.get("transfers").and_then(Value::as_array).cloned().unwrap_or_default();
        let found = arr.iter().find(|t| jstr(t.get("id")) == id.0);
        match found {
            Some(t) => Ok(self.to_transfer(t).await),
            None => Err(DebridError::Provider(format!("transfer {} not found", id.0))),
        }
    }

    async fn unrestrict(&self, link: &RestrictedLink) -> Result<DirectLink> {
        // Premiumize folder links are already direct HTTPS.
        let url = link.0.clone();
        let filename = url.rsplit('/').next().unwrap_or("download").split('?').next().unwrap_or("download").to_string();
        Ok(DirectLink { url, filename, size: 0, mime: None })
    }

    async fn delete(&self, id: &TransferId) -> Result<()> {
        self.post("/transfer/delete", &[("id", &id.0)]).await.map(|_| ())
    }
}

impl Premiumize {
    async fn to_transfer(&self, t: &Value) -> RemoteTransfer {
        let status = pm_status(t.get("status").and_then(Value::as_str).unwrap_or(""));
        let progress = t.get("progress").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let links = if status == TransferStatus::Ready {
            let folder = jstr(t.get("folder_id"));
            if folder.is_empty() { Vec::new() } else { self.folder_links(&folder).await }
        } else {
            Vec::new()
        };
        RemoteTransfer {
            id: TransferId(jstr(t.get("id"))),
            name: jstr(t.get("name")),
            status,
            progress: progress.clamp(0.0, 1.0),
            links,
        }
    }
}
