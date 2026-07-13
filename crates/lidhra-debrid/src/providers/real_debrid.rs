//! Real-Debrid adapter - hits the public REST API v1.0.
//! Docs: <https://api.real-debrid.com/> · base `https://api.real-debrid.com/rest/1.0`.
//!
//! Auth is a Bearer token: either a personal API token (my.real-debrid.com/apitoken)
//! or an OAuth2 access token.

use crate::error::{DebridError, Result};
use crate::model::*;
use crate::provider::DebridProvider;
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::RwLock;

const BASE: &str = "https://api.real-debrid.com/rest/1.0";

pub struct RealDebrid {
    http: reqwest::Client,
    token: RwLock<String>,
}

impl RealDebrid {
    pub fn new(token: impl Into<String>) -> Self {
        RealDebrid {
            http: reqwest::Client::new(),
            token: RwLock::new(token.into()),
        }
    }

    fn token(&self) -> String {
        self.token.read().expect("token lock poisoned").clone()
    }

    /// Turn a non-2xx response into a typed error, reading RD's error body.
    async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
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
}

// ---------- RD response shapes ----------

#[derive(Deserialize)]
struct RdUser {
    username: String,
    #[serde(rename = "type")]
    kind: String, // "premium" | "free"
    premium: Option<i64>, // seconds of premium left
}

#[derive(Deserialize)]
struct RdAdded {
    id: String,
}

#[derive(Deserialize)]
struct RdTorrentInfo {
    id: String,
    filename: String,
    status: String,
    #[serde(default)]
    progress: f32, // 0-100
    #[serde(default)]
    links: Vec<String>,
}

#[derive(Deserialize)]
struct RdUnrestrict {
    download: String,
    filename: String,
    #[serde(default)]
    filesize: u64,
    #[serde(rename = "mimeType", default)]
    mime_type: Option<String>,
}

fn map_status(s: &str, progress: f32) -> TransferStatus {
    match s {
        "downloaded" => TransferStatus::Ready,
        "downloading" | "compressing" | "uploading" => TransferStatus::Downloading,
        "queued" | "magnet_conversion" | "waiting_files_selection" => TransferStatus::Queued,
        "error" | "virus" | "dead" | "magnet_error" => TransferStatus::Error,
        _ if progress >= 100.0 => TransferStatus::Ready,
        _ => TransferStatus::Queued,
    }
}

impl From<RdTorrentInfo> for RemoteTransfer {
    fn from(t: RdTorrentInfo) -> Self {
        RemoteTransfer {
            id: TransferId(t.id),
            name: t.filename,
            status: map_status(&t.status, t.progress),
            progress: (t.progress / 100.0).clamp(0.0, 1.0),
            links: t.links.into_iter().map(RestrictedLink).collect(),
        }
    }
}

#[async_trait]
impl DebridProvider for RealDebrid {
    fn id(&self) -> ProviderId {
        ProviderId::RealDebrid
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            magnet: true,
            torrent_file: true,
            batch_cache_check: true,
            streaming_link: true,
            folders: true,
        }
    }

    async fn authenticate(&self, cred: Credential) -> Result<()> {
        let token = match cred {
            Credential::ApiKey(k) => k,
            Credential::OAuth { access, .. } => access,
        };
        *self.token.write().expect("token lock poisoned") = token;
        // Validate by fetching the account.
        self.account().await.map(|_| ())
    }

    async fn account(&self) -> Result<AccountInfo> {
        let resp = self
            .http
            .get(format!("{BASE}/user"))
            .bearer_auth(self.token())
            .send()
            .await?;
        let user: RdUser = Self::check(resp).await?.json().await?;
        Ok(AccountInfo {
            username: user.username,
            premium: user.kind == "premium",
            expires_at: user.premium, // seconds remaining (RD does not return an absolute ts here)
            traffic_left: None,
        })
    }

    async fn check_cache(&self, hashes: &[InfoHash]) -> Result<Vec<CacheStatus>> {
        // NOTE: RD deprecated /torrents/instantAvailability in 2024 (now returns
        // empty). We keep the call for compatibility but treat "unknown" as not
        // cached, so callers fall back to add + poll. Kept honest on purpose.
        let mut out = Vec::with_capacity(hashes.len());
        for h in hashes {
            let resp = self
                .http
                .get(format!("{BASE}/torrents/instantAvailability/{h}"))
                .bearer_auth(self.token())
                .send()
                .await;
            let cached = match resp {
                Ok(r) if r.status().is_success() => {
                    let v: serde_json::Value = r.json().await.unwrap_or(serde_json::Value::Null);
                    // Non-empty object for the hash key ⇒ cached.
                    v.get(h.as_hex())
                        .and_then(|x| x.as_object())
                        .map(|o| !o.is_empty())
                        .unwrap_or(false)
                }
                _ => false,
            };
            out.push(CacheStatus { hash: h.clone(), cached });
        }
        Ok(out)
    }

    async fn add_magnet(&self, magnet: &Magnet) -> Result<RemoteTransfer> {
        // 1) add the magnet
        let resp = self
            .http
            .post(format!("{BASE}/torrents/addMagnet"))
            .bearer_auth(self.token())
            .form(&[("magnet", magnet.uri.as_str())])
            .send()
            .await?;
        let added: RdAdded = Self::check(resp).await?.json().await?;
        // 2) select all files so the cloud download starts
        self.select_all(&added.id).await?;
        // 3) return current state
        self.transfer(&TransferId(added.id)).await
    }

    async fn add_torrent(&self, torrent: &[u8]) -> Result<RemoteTransfer> {
        let resp = self
            .http
            .put(format!("{BASE}/torrents/addTorrent"))
            .bearer_auth(self.token())
            .body(torrent.to_vec())
            .send()
            .await?;
        let added: RdAdded = Self::check(resp).await?.json().await?;
        self.select_all(&added.id).await?;
        self.transfer(&TransferId(added.id)).await
    }

    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>> {
        let resp = self
            .http
            .get(format!("{BASE}/torrents"))
            .bearer_auth(self.token())
            .send()
            .await?;
        let items: Vec<RdTorrentInfo> = Self::check(resp).await?.json().await?;
        Ok(items.into_iter().map(Into::into).collect())
    }

    async fn transfer(&self, id: &TransferId) -> Result<RemoteTransfer> {
        let resp = self
            .http
            .get(format!("{BASE}/torrents/info/{}", id.0))
            .bearer_auth(self.token())
            .send()
            .await?;
        let info: RdTorrentInfo = Self::check(resp).await?.json().await?;
        Ok(info.into())
    }

    async fn unrestrict(&self, link: &RestrictedLink) -> Result<DirectLink> {
        let resp = self
            .http
            .post(format!("{BASE}/unrestrict/link"))
            .bearer_auth(self.token())
            .form(&[("link", link.0.as_str())])
            .send()
            .await?;
        let u: RdUnrestrict = Self::check(resp).await?.json().await?;
        Ok(DirectLink {
            url: u.download,
            filename: u.filename,
            size: u.filesize,
            mime: u.mime_type,
        })
    }

    async fn delete(&self, id: &TransferId) -> Result<()> {
        let resp = self
            .http
            .delete(format!("{BASE}/torrents/delete/{}", id.0))
            .bearer_auth(self.token())
            .send()
            .await?;
        Self::check(resp).await.map(|_| ())
    }
}

impl RealDebrid {
    async fn select_all(&self, id: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{BASE}/torrents/selectFiles/{id}"))
            .bearer_auth(self.token())
            .form(&[("files", "all")])
            .send()
            .await?;
        Self::check(resp).await.map(|_| ())
    }
}
