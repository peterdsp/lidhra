//! Device login for screens without a keyboard (TVs, set-top boxes).
//!
//! The TV shows a short code and a URL; the user approves it on a phone or a
//! computer; the TV polls until the provider hands back a credential. No token
//! is ever typed with a remote.
//!
//! - **Real-Debrid**: OAuth2 device flow for open-source apps
//!   (<https://api.real-debrid.com/>, client id `X245A4XAIBGVM`). Polling
//!   `device/credentials` yields a user-bound client id + secret, which are then
//!   exchanged for an access token and a long-lived refresh token.
//! - **AllDebrid**: PIN flow (`v4.1/pin/get`, then `v4/pin/check`), which yields
//!   a regular API key.
//!
//! Other providers only offer pasted API keys; [`supports`] says which is which.

use crate::error::{DebridError, Result};
use crate::model::ProviderId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const RD_OAUTH: &str = "https://api.real-debrid.com/oauth/v2";
/// Real-Debrid's published client id for open-source apps.
const RD_OPEN_SOURCE_CLIENT_ID: &str = "X245A4XAIBGVM";
const RD_DEVICE_GRANT: &str = "http://oauth.net/grant_type/device/1.0";
const AD_BASE: &str = "https://api.alldebrid.com";
const AD_AGENT: &str = "lidhra";

/// Whether `id` supports [`start`] / [`poll`].
pub fn supports(id: ProviderId) -> bool {
    matches!(id, ProviderId::RealDebrid | ProviderId::AllDebrid)
}

/// What the TV shows while waiting for approval.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DeviceLogin {
    /// Short code the user types on the approval page.
    pub user_code: String,
    /// Page the user opens on another device.
    pub verify_url: String,
    /// Seconds to wait between [`poll`] calls.
    pub interval: u64,
    /// Seconds until the code expires.
    pub expires_in: u64,
    /// Opaque state for [`poll`] (JSON); keep it, don't show it.
    pub handle: String,
}

/// Refresh material for Real-Debrid OAuth tokens (access tokens expire).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OAuthRefresh {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}

/// The credential a finished device login produced.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DeviceCredential {
    /// Bearer token or API key, ready for [`crate::build_provider`].
    pub token: String,
    /// Present for OAuth providers whose token must be refreshed.
    pub oauth: Option<OAuthRefresh>,
}

#[derive(Debug, PartialEq)]
pub enum PollOutcome {
    /// Not approved yet; poll again after `interval` seconds.
    Pending,
    Done(DeviceCredential),
}

fn http() -> reqwest::Client {
    reqwest::Client::new()
}

fn num(v: &Value, k: &str) -> u64 {
    match v.get(k) {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn text(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Begin a device login. Errors for providers without one (see [`supports`]).
pub async fn start(id: ProviderId) -> Result<DeviceLogin> {
    match id {
        ProviderId::RealDebrid => rd_start().await,
        ProviderId::AllDebrid => ad_start().await,
        other => Err(DebridError::Provider(format!("{}: no device login, paste an API key", other.label()))),
    }
}

/// Check whether the user approved the code yet.
pub async fn poll(id: ProviderId, handle: &str) -> Result<PollOutcome> {
    let h: Value = serde_json::from_str(handle)?;
    match id {
        ProviderId::RealDebrid => rd_poll(&h).await,
        ProviderId::AllDebrid => ad_poll(&h).await,
        other => Err(DebridError::Provider(format!("{}: no device login", other.label()))),
    }
}

// ---------- Real-Debrid ----------

async fn rd_start() -> Result<DeviceLogin> {
    let v: Value = http()
        .get(format!("{RD_OAUTH}/device/code"))
        .query(&[("client_id", RD_OPEN_SOURCE_CLIENT_ID), ("new_credentials", "yes")])
        .send()
        .await?
        .json()
        .await?;
    rd_login_from(&v)
}

fn rd_login_from(v: &Value) -> Result<DeviceLogin> {
    let device_code = text(v, "device_code");
    if device_code.is_empty() {
        return Err(DebridError::Decode("Real-Debrid: no device_code".into()));
    }
    Ok(DeviceLogin {
        user_code: text(v, "user_code"),
        verify_url: {
            let u = text(v, "verification_url");
            if u.is_empty() { "https://real-debrid.com/device".into() } else { u }
        },
        interval: num(v, "interval").max(1),
        expires_in: num(v, "expires_in"),
        handle: serde_json::json!({ "device_code": device_code }).to_string(),
    })
}

async fn rd_poll(h: &Value) -> Result<PollOutcome> {
    let code = text(h, "device_code");
    // Until the user approves, this answers with an error body / non-2xx: that's "pending".
    let resp = http()
        .get(format!("{RD_OAUTH}/device/credentials"))
        .query(&[("client_id", RD_OPEN_SOURCE_CLIENT_ID), ("code", code.as_str())])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Ok(PollOutcome::Pending);
    }
    let creds: Value = resp.json().await?;
    let (client_id, client_secret) = (text(&creds, "client_id"), text(&creds, "client_secret"));
    if client_id.is_empty() || client_secret.is_empty() {
        return Ok(PollOutcome::Pending);
    }
    let tok = rd_token(&client_id, &client_secret, &code).await?;
    Ok(PollOutcome::Done(tok))
}

/// Exchange `code` (a device code, or a refresh token) for fresh tokens.
async fn rd_token(client_id: &str, client_secret: &str, code: &str) -> Result<DeviceCredential> {
    let resp = http()
        .post(format!("{RD_OAUTH}/token"))
        .form(&[("client_id", client_id), ("client_secret", client_secret), ("code", code), ("grant_type", RD_DEVICE_GRANT)])
        .send()
        .await?;
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    let access = text(&v, "access_token");
    if !status.is_success() || access.is_empty() {
        return Err(if matches!(status.as_u16(), 400 | 401 | 403) {
            DebridError::Auth
        } else {
            DebridError::Provider(format!("Real-Debrid token: {status}"))
        });
    }
    Ok(DeviceCredential {
        token: access,
        oauth: Some(OAuthRefresh {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            refresh_token: text(&v, "refresh_token"),
        }),
    })
}

/// Trade a stored refresh token for a new access token (Real-Debrid).
pub async fn refresh(r: &OAuthRefresh) -> Result<DeviceCredential> {
    rd_token(&r.client_id, &r.client_secret, &r.refresh_token).await
}

// ---------- AllDebrid ----------

async fn ad_start() -> Result<DeviceLogin> {
    let v: Value = http()
        .get(format!("{AD_BASE}/v4.1/pin/get"))
        .query(&[("agent", AD_AGENT)])
        .send()
        .await?
        .json()
        .await?;
    ad_login_from(&v)
}

fn ad_login_from(v: &Value) -> Result<DeviceLogin> {
    let d = v.get("data").unwrap_or(v);
    let (pin, check) = (text(d, "pin"), text(d, "check"));
    if pin.is_empty() || check.is_empty() {
        let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("no pin returned");
        return Err(DebridError::Provider(format!("AllDebrid: {msg}")));
    }
    Ok(DeviceLogin {
        user_code: pin.clone(),
        verify_url: {
            let u = text(d, "base_url");
            if u.is_empty() { "https://alldebrid.com/pin/".into() } else { u }
        },
        interval: 5,
        expires_in: num(d, "expires_in"),
        handle: serde_json::json!({ "pin": pin, "check": check }).to_string(),
    })
}

async fn ad_poll(h: &Value) -> Result<PollOutcome> {
    let (pin, check) = (text(h, "pin"), text(h, "check"));
    let v: Value = http()
        .post(format!("{AD_BASE}/v4/pin/check"))
        .query(&[("agent", AD_AGENT)])
        .form(&[("pin", pin.as_str()), ("check", check.as_str())])
        .send()
        .await?
        .json()
        .await?;
    ad_outcome(&v)
}

fn ad_outcome(v: &Value) -> Result<PollOutcome> {
    if v.get("status").and_then(Value::as_str) == Some("error") {
        let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("pin check failed");
        return Err(DebridError::Provider(format!("AllDebrid: {msg}")));
    }
    let d = v.get("data").unwrap_or(v);
    let key = text(d, "apikey");
    if d.get("activated").and_then(Value::as_bool) == Some(true) && !key.is_empty() {
        Ok(PollOutcome::Done(DeviceCredential { token: key, oauth: None }))
    } else {
        Ok(PollOutcome::Pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn support_matrix() {
        assert!(supports(ProviderId::RealDebrid));
        assert!(supports(ProviderId::AllDebrid));
        assert!(!supports(ProviderId::TorBox));
    }

    #[test]
    fn parses_real_debrid_device_code() {
        let l = rd_login_from(&json!({
            "device_code": "DEV123", "user_code": "ABCD1234", "interval": 5, "expires_in": 1800,
            "verification_url": "https://real-debrid.com/device"
        }))
        .unwrap();
        assert_eq!(l.user_code, "ABCD1234");
        assert_eq!(l.interval, 5);
        assert_eq!(l.expires_in, 1800);
        let h: Value = serde_json::from_str(&l.handle).unwrap();
        assert_eq!(h["device_code"], "DEV123");
        assert!(rd_login_from(&json!({})).is_err());
    }

    #[test]
    fn parses_alldebrid_pin() {
        let l = ad_login_from(&json!({ "status": "success", "data": {
            "pin": "Z9Y8", "check": "c0ffee", "expires_in": 600,
            "user_url": "https://alldebrid.com/pin/?pin=Z9Y8", "base_url": "https://alldebrid.com/pin/"
        }}))
        .unwrap();
        assert_eq!(l.user_code, "Z9Y8");
        assert_eq!(l.verify_url, "https://alldebrid.com/pin/");
        assert!(ad_login_from(&json!({ "status": "error", "error": { "message": "nope" } })).is_err());

        assert_eq!(ad_outcome(&json!({ "status": "success", "data": { "activated": false } })).unwrap(), PollOutcome::Pending);
        assert_eq!(
            ad_outcome(&json!({ "status": "success", "data": { "activated": true, "apikey": "KEY" } })).unwrap(),
            PollOutcome::Done(DeviceCredential { token: "KEY".into(), oauth: None })
        );
        assert!(ad_outcome(&json!({ "status": "error", "error": { "message": "expired" } })).is_err());
    }
}
