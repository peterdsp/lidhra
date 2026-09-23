//! Deepbrid adapter - API v1. Docs: <https://www.deepbrid.com/api-docs>.
//! Base `https://www.deepbrid.com/api/v1`, auth is a Bearer API key (Devices & API page).
//! Responses are bare JSON; failures carry `{ "error": <http-ish code>, "message": "…" }`
//! (`error` is `0` on success). Torrent status strings mirror Real-Debrid's.
//!
//! Torrent `links` are restricted hoster-style URLs and go through
//! `generate/link` like any other link. JSON-navigated; verify field paths
//! against a live account.

use super::util::{check_status, filename_from_url, jnum, jstr, parse_size, parse_ymd, secret};
use crate::error::{DebridError, Result};
use crate::model::*;
use crate::provider::DebridProvider;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::RwLock;

const BASE: &str = "https://www.deepbrid.com/api/v1";

pub struct Deepbrid {
    http: reqwest::Client,
    apikey: RwLock<String>,
}

impl Deepbrid {
    pub fn new(apikey: impl Into<String>) -> Self {
        Deepbrid { http: reqwest::Client::new(), apikey: RwLock::new(apikey.into()) }
    }

    fn key(&self) -> String {
        self.apikey.read().expect("key lock").clone()
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value> {
        let resp = req.bearer_auth(self.key()).send().await?;
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
        match v.get("error") {
            None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(v),
            Some(e) => {
                let code = jnum(Some(e));
                if e.is_number() && code == 0.0 {
                    return Ok(v);
                }
                let msg = jstr(v.get("message"));
                Err(match code as u16 {
                    401 | 403 => DebridError::Auth,
                    429 => DebridError::RateLimited,
                    _ if msg.is_empty() => DebridError::Provider(jstr(Some(e))),
                    _ => DebridError::Provider(msg),
                })
            }
        }
    }
}

/// Same vocabulary as Real-Debrid.
fn db_status(s: &str, progress: f32) -> TransferStatus {
    match s {
        "downloaded" => TransferStatus::Ready,
        "downloading" | "compressing" | "uploading" => TransferStatus::Downloading,
        "queued" | "magnet_conversion" | "waiting_files_selection" => TransferStatus::Queued,
        "error" | "virus" | "dead" | "magnet_error" => TransferStatus::Error,
        _ if progress >= 100.0 => TransferStatus::Ready,
        _ => TransferStatus::Queued,
    }
}

fn db_transfer(t: &Value) -> RemoteTransfer {
    let progress = jnum(t.get("progress")) as f32;
    let links = t
        .get("links")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|l| if l.is_string() { jstr(Some(l)) } else { jstr(l.get("link")) })
                .filter(|s| !s.is_empty())
                .map(RestrictedLink)
                .collect()
        })
        .unwrap_or_default();
    RemoteTransfer {
        id: TransferId(jstr(t.get("id"))),
        name: jstr(t.get("filename")),
        status: db_status(&jstr(t.get("status")), progress),
        progress: (progress / 100.0).clamp(0.0, 1.0),
        links,
    }
}

/// `torrents/info` without an id returns an object keyed by row number (or a list).
fn db_rows(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o.values().filter(|x| x.is_object()).cloned().collect(),
        _ => Vec::new(),
    }
}

#[async_trait]
impl DebridProvider for Deepbrid {
    fn id(&self) -> ProviderId {
        ProviderId::Deepbrid
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { magnet: true, torrent_file: false, batch_cache_check: false, streaming_link: true, folders: false }
    }

    async fn authenticate(&self, cred: Credential) -> Result<()> {
        *self.apikey.write().expect("key lock") = secret(cred);
        self.account().await.map(|_| ())
    }

    async fn account(&self) -> Result<AccountInfo> {
        let v = self.get("/user", &[]).await?;
        let username = {
            let u = jstr(v.get("username"));
            if u.is_empty() { jstr(v.get("email")) } else { u }
        };
        Ok(AccountInfo {
            username,
            premium: jstr(v.get("type")).eq_ignore_ascii_case("premium"),
            expires_at: parse_ymd(&jstr(v.get("expiration"))),
            traffic_left: None,
        })
    }

    async fn check_cache(&self, hashes: &[InfoHash]) -> Result<Vec<CacheStatus>> {
        // No cache endpoint documented; report unknown → callers add + poll.
        Ok(hashes.iter().cloned().map(|hash| CacheStatus { hash, cached: false }).collect())
    }

    async fn add_magnet(&self, magnet: &Magnet) -> Result<RemoteTransfer> {
        let v = self.post_form("/torrents/add", &[("magnet", &magnet.uri)]).await?;
        let id = jstr(v.get("id"));
        if id.is_empty() {
            return Err(DebridError::Provider("Deepbrid: no torrent id returned".into()));
        }
        self.transfer(&TransferId(id)).await
    }

    async fn add_torrent(&self, _torrent: &[u8]) -> Result<RemoteTransfer> {
        Err(DebridError::Provider("Deepbrid: use add_magnet (file upload not implemented)".into()))
    }

    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>> {
        let v = self.get("/torrents/info", &[]).await?;
        Ok(db_rows(&v).iter().map(db_transfer).collect())
    }

    async fn transfer(&self, id: &TransferId) -> Result<RemoteTransfer> {
        let v = self.get("/torrents/info", &[("id", &id.0)]).await?;
        let t = if jstr(v.get("id")).is_empty() {
            db_rows(&v).into_iter().find(|r| jstr(r.get("id")) == id.0)
        } else {
            Some(v)
        };
        t.map(|t| db_transfer(&t)).ok_or_else(|| DebridError::Provider(format!("transfer {} not found", id.0)))
    }

    async fn unrestrict(&self, link: &RestrictedLink) -> Result<DirectLink> {
        let v = self.post_form("/generate/link", &[("link", &link.0)]).await?;
        let url = jstr(v.get("link"));
        if url.is_empty() {
            return Err(DebridError::Provider("Deepbrid: no link returned".into()));
        }
        let filename = {
            let f = jstr(v.get("filename"));
            if f.is_empty() { filename_from_url(&url) } else { f }
        };
        Ok(DirectLink { url, filename, size: parse_size(v.get("size")), mime: None })
    }

    async fn delete(&self, id: &TransferId) -> Result<()> {
        self.send(self.http.delete(format!("{BASE}/torrents/delete/{}", id.0))).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unwraps_envelope() {
        assert!(Deepbrid::unwrap(json!({ "error": 0, "message": "OK", "id": "1" })).is_ok());
        assert!(Deepbrid::unwrap(json!({ "username": "x" })).is_ok());
        assert!(matches!(Deepbrid::unwrap(json!({ "error": 401, "message": "Authentication required." })), Err(DebridError::Auth)));
        assert!(matches!(Deepbrid::unwrap(json!({ "error": 429 })), Err(DebridError::RateLimited)));
        assert!(matches!(Deepbrid::unwrap(json!({ "error": 500, "message": "boom" })), Err(DebridError::Provider(m)) if m == "boom"));
    }

    #[test]
    fn maps_torrents() {
        let t = json!({ "id": "14648", "filename": "ubuntu.iso", "progress": 100, "status": "downloaded",
                        "links": ["https://www.deepbrid.com/mytorrents?torrent=14648&file=Kx7mPq"] });
        let r = db_transfer(&t);
        assert_eq!(r.status, TransferStatus::Ready);
        assert_eq!(r.progress, 1.0);
        assert_eq!(r.links.len(), 1);
        assert_eq!(db_status("magnet_conversion", 0.0), TransferStatus::Queued);
        assert_eq!(db_status("virus", 10.0), TransferStatus::Error);
        // list shape: keyed by row number
        let rows = db_rows(&json!({ "1": { "id": "a" }, "2": { "id": "b" } }));
        assert_eq!(rows.len(), 2);
        assert_eq!(db_rows(&json!([{ "id": "a" }])).len(), 1);
    }
}
