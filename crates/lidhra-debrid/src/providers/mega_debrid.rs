//! Mega-Debrid adapter. Docs: <https://www.mega-debrid.eu/index.php?page=api>.
//! Base `https://www.mega-debrid.eu/api.php?action=<action>&token=<token>`.
//! Envelope: `{ "response_code": "ok", ... }` or `{ "response_code": "…", "response_text": "…" }`.
//!
//! Mega-Debrid has no long-lived API key: `connectUser` trades login + password
//! for a short-lived token that dies on the next login or a "Token error". So the
//! credential is either `login:password` (we log in and transparently renew) or a
//! raw token pasted from the web app (works until it expires; no renewal possible).
//!
//! Torrents are addressed by info-hash (`uploadTorrent` returns it, `getTorrent`
//! takes it), so [`TransferId`] holds the hash. The API has no delete action.
//! JSON-navigated; verify field paths against a live account.

use super::util::{check_status, filename_from_url, jnum, jstr, now_unix, secret};
use crate::error::{DebridError, Result};
use crate::model::*;
use crate::provider::DebridProvider;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::RwLock;

const BASE: &str = "https://www.mega-debrid.eu/api.php";

#[derive(Clone, Default)]
struct Session {
    /// `Some((login, password))` when we can renew the token ourselves.
    login: Option<(String, String)>,
    token: String,
    /// From `connectUser`; unknown for a pasted token.
    vip_end: Option<i64>,
    email: String,
}

pub struct MegaDebrid {
    http: reqwest::Client,
    session: RwLock<Session>,
}

/// `login:password` → renewable session; anything else is treated as a token.
fn session_from_secret(secret: &str) -> Session {
    match secret.split_once(':') {
        Some((login, password)) if !login.is_empty() && !password.is_empty() => Session {
            login: Some((login.to_string(), password.to_string())),
            ..Session::default()
        },
        _ => Session { token: secret.to_string(), ..Session::default() },
    }
}

impl MegaDebrid {
    pub fn new(secret: impl Into<String>) -> Self {
        MegaDebrid { http: reqwest::Client::new(), session: RwLock::new(session_from_secret(&secret.into())) }
    }

    fn session(&self) -> Session {
        self.session.read().expect("session lock").clone()
    }

    /// `connectUser`: swap login + password for a fresh token.
    async fn connect(&self) -> Result<Session> {
        let mut s = self.session();
        let (login, password) = s.login.clone().ok_or(DebridError::Auth)?;
        let resp = self
            .http
            .get(BASE)
            .query(&[("action", "connectUser"), ("login", &login), ("password", &password)])
            .send()
            .await?;
        let v: Value = check_status(resp).await?.json().await?;
        if jstr(v.get("response_code")) != "ok" {
            return Err(DebridError::Auth);
        }
        s.token = jstr(v.get("token"));
        s.vip_end = v.get("vip_end").map(|x| jnum(Some(x)) as i64).filter(|&t| t > 0);
        s.email = jstr(v.get("email"));
        if s.token.is_empty() {
            return Err(DebridError::Auth);
        }
        *self.session.write().expect("session lock") = s.clone();
        Ok(s)
    }

    /// One API action. Renews the token once on "Token error" when we hold a login.
    async fn call(&self, method: reqwest::Method, action: &str, form: &[(&str, &str)]) -> Result<Value> {
        let mut s = self.session();
        if s.token.is_empty() {
            s = self.connect().await?;
        }
        match self.call_with(&s.token, method.clone(), action, form).await {
            Err(DebridError::Auth) if s.login.is_some() => {
                let s = self.connect().await?;
                self.call_with(&s.token, method, action, form).await
            }
            other => other,
        }
    }

    async fn call_with(&self, token: &str, method: reqwest::Method, action: &str, form: &[(&str, &str)]) -> Result<Value> {
        let req = self.http.request(method.clone(), BASE).query(&[("action", action), ("token", token)]);
        let req = if method == reqwest::Method::POST { req.form(form) } else { req.query(form) };
        let v: Value = check_status(req.send().await?).await?.json().await?;
        Self::unwrap(v)
    }

    fn unwrap(v: Value) -> Result<Value> {
        let code = jstr(v.get("response_code"));
        if code == "ok" {
            return Ok(v);
        }
        let text = jstr(v.get("response_text"));
        let lower = text.to_ascii_lowercase();
        if lower.contains("token") || lower.contains("log-in") || lower.contains("login") {
            Err(DebridError::Auth)
        } else if text.is_empty() {
            Err(DebridError::Provider(code))
        } else {
            Err(DebridError::Provider(format!("{code}: {text}")))
        }
    }
}

/// Progress may be `45`, `"45"` or `"45%"`.
fn md_progress(v: Option<&Value>) -> f32 {
    let p = match v {
        Some(Value::String(s)) => s.trim().trim_end_matches('%').parse().unwrap_or(0.0),
        other => jnum(other),
    };
    (p / 100.0).clamp(0.0, 1.0) as f32
}

/// The docs don't enumerate `status` values, so match on stems and fall back to progress.
fn md_status(status: &str, progress: f32) -> TransferStatus {
    let s = status.to_ascii_lowercase();
    if s.contains("error") || s.contains("fail") || s.contains("dead") {
        TransferStatus::Error
    } else if s.contains("complet") || s.contains("finish") || s.contains("done") || s.contains("seed") || progress >= 1.0 {
        TransferStatus::Ready
    } else if s.contains("queue") || s.contains("wait") || s.contains("pending") {
        TransferStatus::Queued
    } else {
        TransferStatus::Downloading
    }
}

fn md_transfer(hash: &str, t: &Value) -> RemoteTransfer {
    let progress = md_progress(t.get("progress"));
    let status = md_status(&jstr(t.get("status")), progress);
    let ub = jstr(t.get("ub_link"));
    let links = if status == TransferStatus::Ready && !ub.is_empty() { vec![RestrictedLink(ub)] } else { Vec::new() };
    RemoteTransfer { id: TransferId(hash.to_string()), name: jstr(t.get("name")), status, progress, links }
}

#[async_trait]
impl DebridProvider for MegaDebrid {
    fn id(&self) -> ProviderId {
        ProviderId::MegaDebrid
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { magnet: true, torrent_file: false, batch_cache_check: false, streaming_link: false, folders: false }
    }

    async fn authenticate(&self, cred: Credential) -> Result<()> {
        *self.session.write().expect("session lock") = session_from_secret(&secret(cred));
        self.account().await.map(|_| ())
    }

    async fn account(&self) -> Result<AccountInfo> {
        if self.session().login.is_some() {
            let s = self.connect().await?;
            return Ok(AccountInfo {
                username: s.email,
                premium: s.vip_end.map(|t| t > now_unix()).unwrap_or(false),
                expires_at: s.vip_end,
                traffic_left: None,
            });
        }
        // Token only: no account action exists, so validate it with a cheap call.
        self.call(reqwest::Method::GET, "getTorrents", &[]).await?;
        Ok(AccountInfo { username: String::new(), premium: true, expires_at: None, traffic_left: None })
    }

    async fn check_cache(&self, hashes: &[InfoHash]) -> Result<Vec<CacheStatus>> {
        // No cache endpoint; report unknown → callers add + poll.
        Ok(hashes.iter().cloned().map(|hash| CacheStatus { hash, cached: false }).collect())
    }

    async fn add_magnet(&self, magnet: &Magnet) -> Result<RemoteTransfer> {
        let v = self.call(reqwest::Method::POST, "uploadTorrent", &[("magnet", &magnet.uri)]).await?;
        let hash = {
            let h = jstr(v.pointer("/newTorrent/hash")).to_ascii_lowercase();
            if h.is_empty() { magnet.hash.as_hex().to_string() } else { h }
        };
        self.transfer(&TransferId(hash)).await
    }

    async fn add_torrent(&self, _torrent: &[u8]) -> Result<RemoteTransfer> {
        Err(DebridError::Provider("Mega-Debrid: use add_magnet (file upload not implemented)".into()))
    }

    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>> {
        let v = self.call(reqwest::Method::GET, "getTorrents", &[]).await?;
        let items: Vec<Value> = match v.get("torrents") {
            Some(Value::Array(a)) => a.clone(),
            Some(Value::Object(o)) => o.values().cloned().collect(),
            _ => Vec::new(),
        };
        Ok(items
            .iter()
            .map(|t| {
                let hash = {
                    let h = jstr(t.get("hash"));
                    if h.is_empty() { jstr(t.get("id")) } else { h }
                };
                md_transfer(&hash.to_ascii_lowercase(), t)
            })
            .collect())
    }

    async fn transfer(&self, id: &TransferId) -> Result<RemoteTransfer> {
        let v = self.call(reqwest::Method::POST, "getTorrent", &[("hash", &id.0)]).await?;
        let t = v.get("status").filter(|s| s.is_object()).unwrap_or(&v);
        Ok(md_transfer(&id.0, t))
    }

    async fn unrestrict(&self, link: &RestrictedLink) -> Result<DirectLink> {
        let v = self.call(reqwest::Method::POST, "getLink", &[("link", &link.0)]).await?;
        let url = jstr(v.get("debridLink"));
        if url.is_empty() {
            return Err(DebridError::Provider("Mega-Debrid: no debridLink returned".into()));
        }
        Ok(DirectLink { filename: filename_from_url(&url), url, size: 0, mime: None })
    }

    async fn delete(&self, _id: &TransferId) -> Result<()> {
        Err(DebridError::Provider("Mega-Debrid: the API has no delete action (remove it in the web app)".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn splits_credentials() {
        let s = session_from_secret("me@example.com:hunter2");
        assert_eq!(s.login, Some(("me@example.com".into(), "hunter2".into())));
        assert!(s.token.is_empty());
        let t = session_from_secret("abcdef123456");
        assert!(t.login.is_none());
        assert_eq!(t.token, "abcdef123456");
        assert!(session_from_secret(":x").login.is_none());
    }

    #[test]
    fn maps_status_and_progress() {
        assert_eq!(md_progress(Some(&json!("45%"))), 0.45);
        assert_eq!(md_progress(Some(&json!(100))), 1.0);
        assert_eq!(md_status("complete", 1.0), TransferStatus::Ready);
        assert_eq!(md_status("downloading", 0.3), TransferStatus::Downloading);
        assert_eq!(md_status("something", 1.0), TransferStatus::Ready);
        assert_eq!(md_status("queued", 0.0), TransferStatus::Queued);
        assert_eq!(md_status("error", 0.5), TransferStatus::Error);
        let t = md_transfer("abc", &json!({ "name": "n", "status": "complete", "progress": "100", "ub_link": "https://x/y" }));
        assert_eq!(t.links.len(), 1);
        assert_eq!(t.id.0, "abc");
    }

    #[test]
    fn maps_errors() {
        assert!(MegaDebrid::unwrap(json!({ "response_code": "ok" })).is_ok());
        assert!(matches!(
            MegaDebrid::unwrap(json!({ "response_code": "TOKEN_ERROR", "response_text": "Token error, please log-in" })),
            Err(DebridError::Auth)
        ));
        assert!(matches!(
            MegaDebrid::unwrap(json!({ "response_code": "UNKNOWN", "response_text": "Unsupported host" })),
            Err(DebridError::Provider(_))
        ));
    }
}
