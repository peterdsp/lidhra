//! TorBox adapter - API v1. Docs: <https://api-docs.torbox.app/>.
//! Auth is a Bearer API key. Envelope: `{ "success": true, "data": ... }`.
//!
//! TorBox addresses files as (torrent_id, file_id) and hands out a download URL
//! per file via `requestdl`. We encode a restricted link as `"<torrent_id>:<file_id>"`
//! so it fits the uniform `unrestrict()` step. Verify paths against a live account.

use crate::error::{DebridError, Result};
use crate::model::*;
use crate::provider::DebridProvider;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::RwLock;

const BASE: &str = "https://api.torbox.app/v1/api";

pub struct TorBox {
    http: reqwest::Client,
    apikey: RwLock<String>,
}

impl TorBox {
    pub fn new(apikey: impl Into<String>) -> Self {
        TorBox { http: reqwest::Client::new(), apikey: RwLock::new(apikey.into()) }
    }
    fn key(&self) -> String {
        self.apikey.read().expect("key lock").clone()
    }

    async fn get(&self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        let v: Value = self
            .http
            .get(format!("{BASE}{path}"))
            .bearer_auth(self.key())
            .query(params)
            .send()
            .await?
            .json()
            .await?;
        Self::unwrap(v)
    }

    async fn post_form(&self, path: &str, form: &[(&str, &str)]) -> Result<Value> {
        let v: Value = self
            .http
            .post(format!("{BASE}{path}"))
            .bearer_auth(self.key())
            .form(form)
            .send()
            .await?
            .json()
            .await?;
        Self::unwrap(v)
    }

    fn unwrap(v: Value) -> Result<Value> {
        if v.get("success").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("data").cloned().unwrap_or(Value::Null))
        } else {
            let msg = v
                .get("detail")
                .and_then(Value::as_str)
                .or_else(|| v.get("error").and_then(Value::as_str))
                .unwrap_or("error");
            Err(DebridError::Provider(msg.to_string()))
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

fn tb_transfer(t: &Value) -> RemoteTransfer {
    let id = jstr(t.get("id"));
    let finished = t.get("download_finished").and_then(Value::as_bool).unwrap_or(false);
    let state = t.get("download_state").and_then(Value::as_str).unwrap_or("");
    let status = if finished || matches!(state, "completed" | "cached" | "uploading") {
        TransferStatus::Ready
    } else if state.contains("download") || state == "metadl" {
        TransferStatus::Downloading
    } else if matches!(state, "error" | "stalled (no seeds)" | "failed") {
        TransferStatus::Error
    } else {
        TransferStatus::Queued
    };
    let progress = t.get("progress").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    // Each file becomes a "torrent_id:file_id" restricted link.
    let links = t
        .get("files")
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .map(|f| RestrictedLink(format!("{id}:{}", jstr(f.get("id")))))
                .collect()
        })
        .unwrap_or_default();
    RemoteTransfer {
        id: TransferId(id),
        name: jstr(t.get("name")),
        status,
        progress: progress.clamp(0.0, 1.0),
        links,
    }
}

#[async_trait]
impl DebridProvider for TorBox {
    fn id(&self) -> ProviderId {
        ProviderId::TorBox
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { magnet: true, torrent_file: true, batch_cache_check: true, streaming_link: true, folders: true }
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
        let d = self.get("/user/me", &[]).await?;
        let plan = d.get("plan").and_then(Value::as_i64).unwrap_or(0);
        Ok(AccountInfo {
            username: {
                let u = jstr(d.get("email"));
                if u.is_empty() { jstr(d.get("id")) } else { u }
            },
            premium: plan > 0,
            expires_at: None,
            traffic_left: None,
        })
    }

    async fn check_cache(&self, hashes: &[InfoHash]) -> Result<Vec<CacheStatus>> {
        let mut out = Vec::with_capacity(hashes.len());
        for h in hashes {
            let cached = self
                .get("/torrents/checkcached", &[("hash", h.as_hex()), ("format", "object")])
                .await
                .map(|d| !d.is_null() && d.get(h.as_hex()).is_some())
                .unwrap_or(false);
            out.push(CacheStatus { hash: h.clone(), cached });
        }
        Ok(out)
    }

    async fn add_magnet(&self, magnet: &Magnet) -> Result<RemoteTransfer> {
        let d = self.post_form("/torrents/createtorrent", &[("magnet", &magnet.uri)]).await?;
        let id = jstr(d.get("torrent_id"));
        if id.is_empty() {
            return Err(DebridError::Provider("TorBox: no torrent_id returned".into()));
        }
        self.transfer(&TransferId(id)).await
    }

    async fn add_torrent(&self, _torrent: &[u8]) -> Result<RemoteTransfer> {
        Err(DebridError::Provider("TorBox: multipart torrent upload not implemented".into()))
    }

    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>> {
        let d = self.get("/torrents/mylist", &[]).await?;
        let arr = d.as_array().cloned().unwrap_or_default();
        Ok(arr.iter().map(tb_transfer).collect())
    }

    async fn transfer(&self, id: &TransferId) -> Result<RemoteTransfer> {
        let d = self.get("/torrents/mylist", &[("id", &id.0)]).await?;
        // May come back as a single object or a 1-element array.
        let t = if d.is_array() { d.get(0).cloned().unwrap_or(Value::Null) } else { d };
        Ok(tb_transfer(&t))
    }

    async fn unrestrict(&self, link: &RestrictedLink) -> Result<DirectLink> {
        let (tid, fid) = link.0.split_once(':').ok_or_else(|| DebridError::Provider("bad TorBox link".into()))?;
        let d = self
            .get("/torrents/requestdl", &[("token", &self.key()), ("torrent_id", tid), ("file_id", fid)])
            .await?;
        // `data` is the direct URL string.
        let url = match d {
            Value::String(s) => s,
            other => jstr(other.get("url")),
        };
        let filename = url.rsplit('/').next().unwrap_or("download").split('?').next().unwrap_or("download").to_string();
        Ok(DirectLink { url, filename, size: 0, mime: None })
    }

    async fn delete(&self, id: &TransferId) -> Result<()> {
        let tid: i64 = id.0.parse().unwrap_or(0);
        let v: Value = self
            .http
            .post(format!("{BASE}/torrents/controltorrent"))
            .bearer_auth(self.key())
            .json(&serde_json::json!({ "torrent_id": tid, "operation": "delete" }))
            .send()
            .await?
            .json()
            .await?;
        Self::unwrap(v).map(|_| ())
    }
}
