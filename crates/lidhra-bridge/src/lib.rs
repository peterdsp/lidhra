//! # lidhra-bridge
//!
//! One JSON command surface over [`lidhra_debrid`] for TV clients. On a TV,
//! Lidhra is a library + player: connect a provider, list the cloud transfers,
//! resolve a ready transfer's files to direct HTTPS links, play one. Nothing is
//! downloaded to the TV.
//!
//! The same [`Bridge::call`] backs two front-ends:
//! - **wasm32** (`wasm` module): a `LidhraBridge` JS class for the packaged web
//!   TVs (Samsung Tizen, LG webOS, HarmonyOS) that have no local server.
//! - **native** (`ffi` module): a C ABI (`include/lidhra_bridge.h`) for the
//!   Apple TV app, which cannot host a webview.
//!
//! Command names and result shapes match the desktop Tauri commands and the
//! `lidhra-server` routes, so the shared web UI talks to all three the same way.
//!
//! | Command | Args | Result |
//! | --- | --- | --- |
//! | `providers` | | `[{id, label, device_login}]` |
//! | `connect` | `{provider, token}` | `{username, premium, provider, session}` |
//! | `login_start` | `{provider}` | `{user_code, verify_url, interval, expires_in, handle}` |
//! | `login_poll` | `{provider, handle}` | `{pending: true}` or `{pending: false, username, premium, provider, session}` |
//! | `restore` | `{session}` | same as `connect`, or `null` without a session |
//! | `session` | | the current session or `null` |
//! | `disconnect` | | `null` |
//! | `transfers` | | `[Tx]` (same shape as the desktop `transfers` command) |
//! | `links` | `{id}` | `[{url, filename, size, mime}]` |
//!
//! The host persists the `session` value it gets back (localStorage, Keychain)
//! and hands it to `restore` on the next launch. Real-Debrid device logins
//! carry OAuth refresh material; the bridge refreshes on `restore` and on an
//! auth error mid-session.

use lidhra_debrid::device_auth::{self, OAuthRefresh, PollOutcome};
use lidhra_debrid::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi;
#[cfg(target_arch = "wasm32")]
pub mod wasm;

/// What a host stores to sign back in without asking again.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Session {
    /// Provider label, e.g. `"Real-Debrid"` (anything [`ProviderId::from_key`] accepts).
    pub provider: String,
    /// API key or OAuth access token.
    pub token: String,
    /// Real-Debrid device logins only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthRefresh>,
}

#[derive(Default)]
struct State {
    provider: Option<Arc<dyn DebridProvider>>,
    session: Option<Session>,
}

/// The command dispatcher. Cheap to create; holds at most one connected provider.
#[derive(Default)]
pub struct Bridge {
    state: Mutex<State>,
}

type Res = std::result::Result<Value, String>;

fn arg<'a>(args: &'a Value, key: &str) -> std::result::Result<&'a str, String> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| format!("missing `{key}`"))
}

fn provider_id(label: &str) -> std::result::Result<ProviderId, String> {
    ProviderId::from_key(label).ok_or_else(|| format!("unknown provider `{label}`"))
}

/// A cloud transfer in the shape the shared UI already renders (desktop `Tx`).
fn tx_json(t: &RemoteTransfer) -> Value {
    json!({
        "id": t.id.0, "name": t.name, "status": format!("{:?}", t.status), "progress": t.progress,
        "links": t.links.len(), "source": "debrid", "downloaded": 0, "total": 0, "down_bps": 0, "up_bps": 0,
        "peers": 0, "known_peers": 0, "eta": null, "error": null, "pause_reason": null, "path": "", "magnet": null
    })
}

async fn sign_in(session: &Session) -> std::result::Result<(Arc<dyn DebridProvider>, AccountInfo), String> {
    let id = provider_id(&session.provider)?;
    let cred = Credential::ApiKey(session.token.clone());
    let p: Arc<dyn DebridProvider> = Arc::from(build_provider(id, cred.clone()).map_err(|e| e.to_string())?);
    p.authenticate(cred).await.map_err(|e| e.to_string())?;
    let a = p.account().await.map_err(|e| e.to_string())?;
    Ok((p, a))
}

/// Swap an OAuth session's access token for a fresh one (refresh tokens are reusable).
async fn refreshed(mut s: Session) -> std::result::Result<Session, String> {
    if let Some(r) = s.oauth.clone() {
        let c = device_auth::refresh(&r).await.map_err(|e| e.to_string())?;
        s.token = c.token;
        if let Some(new) = c.oauth.filter(|n| !n.refresh_token.is_empty()) {
            s.oauth = Some(new);
        }
    }
    Ok(s)
}

impl Bridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run one command. `args` is a JSON object (or `null`).
    pub async fn call(&self, name: &str, args: &Value) -> Res {
        match name {
            "providers" => Ok(Value::Array(
                ProviderId::IMPLEMENTED
                    .iter()
                    .map(|&p| json!({ "id": p.label(), "label": p.label(), "device_login": device_auth::supports(p) }))
                    .collect(),
            )),
            "connect" => {
                let s = Session { provider: arg(args, "provider")?.to_string(), token: arg(args, "token")?.to_string(), oauth: None };
                self.establish(s).await
            }
            "login_start" => {
                let id = provider_id(arg(args, "provider")?)?;
                let l = device_auth::start(id).await.map_err(|e| e.to_string())?;
                serde_json::to_value(l).map_err(|e| e.to_string())
            }
            "login_poll" => {
                let label = arg(args, "provider")?;
                let id = provider_id(label)?;
                match device_auth::poll(id, arg(args, "handle")?).await.map_err(|e| e.to_string())? {
                    PollOutcome::Pending => Ok(json!({ "pending": true })),
                    PollOutcome::Done(c) => {
                        let mut v = self.establish(Session { provider: label.to_string(), token: c.token, oauth: c.oauth }).await?;
                        v["pending"] = json!(false);
                        Ok(v)
                    }
                }
            }
            "restore" => {
                let raw = match args.get("session") {
                    None | Some(Value::Null) => return Ok(Value::Null),
                    Some(Value::String(s)) if s.is_empty() => return Ok(Value::Null),
                    Some(Value::String(s)) => serde_json::from_str::<Value>(s).map_err(|e| format!("bad session: {e}"))?,
                    Some(v) => v.clone(),
                };
                let s: Session = serde_json::from_value(raw).map_err(|e| format!("bad session: {e}"))?;
                self.establish(refreshed(s).await?).await
            }
            "session" => Ok(self.state.lock().unwrap().session.as_ref().map(|s| json!(s)).unwrap_or(Value::Null)),
            "disconnect" => {
                *self.state.lock().unwrap() = State::default();
                Ok(Value::Null)
            }
            "transfers" => {
                let list = match self.provider()?.list_transfers().await {
                    Err(DebridError::Auth) if self.reauth().await => self.provider()?.list_transfers().await,
                    other => other,
                }
                .map_err(|e| e.to_string())?;
                Ok(Value::Array(list.iter().map(tx_json).collect()))
            }
            "links" => {
                let id = TransferId(arg(args, "id")?.to_string());
                match links(self.provider()?, &id).await {
                    Err(DebridError::Auth) if self.reauth().await => links(self.provider()?, &id).await,
                    other => other,
                }
                .map_err(|e| e.to_string())
            }
            other => Err(format!("unknown command `{other}`")),
        }
    }

    fn provider(&self) -> std::result::Result<Arc<dyn DebridProvider>, String> {
        self.state.lock().unwrap().provider.clone().ok_or_else(|| "connect a provider first".to_string())
    }

    /// Sign in with `s`, keep it as the active session, and describe the account.
    async fn establish(&self, s: Session) -> Res {
        let (p, a) = sign_in(&s).await?;
        let out = json!({ "username": a.username, "premium": a.premium, "provider": s.provider, "session": s });
        *self.state.lock().unwrap() = State { provider: Some(p), session: Some(s) };
        Ok(out)
    }

    /// After an auth error: refresh an OAuth session and reconnect. False when that isn't possible.
    async fn reauth(&self) -> bool {
        let Some(s) = self.state.lock().unwrap().session.clone().filter(|s| s.oauth.is_some()) else { return false };
        match refreshed(s).await {
            Ok(s) => self.establish(s).await.is_ok(),
            Err(_) => false,
        }
    }
}

async fn links(p: Arc<dyn DebridProvider>, id: &TransferId) -> lidhra_debrid::Result<Value> {
    let t = p.transfer(id).await?;
    let mut out = Vec::with_capacity(t.links.len());
    for l in &t.links {
        let d = p.unrestrict(l).await?;
        out.push(json!({ "url": d.url, "filename": d.filename, "size": d.size, "mime": d.mime }));
    }
    Ok(Value::Array(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lidhra_debrid::RestrictedLink;

    fn run(b: &Bridge, name: &str, args: Value) -> Res {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(b.call(name, &args))
    }

    #[test]
    fn lists_providers_with_device_login_flags() {
        let v = run(&Bridge::new(), "providers", Value::Null).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), ProviderId::IMPLEMENTED.len());
        let rd = arr.iter().find(|p| p["id"] == "Real-Debrid").unwrap();
        assert_eq!(rd["device_login"], true);
        let tb = arr.iter().find(|p| p["id"] == "TorBox").unwrap();
        assert_eq!(tb["device_login"], false);
    }

    #[test]
    fn rejects_bad_input_without_network() {
        let b = Bridge::new();
        assert!(run(&b, "nope", Value::Null).unwrap_err().contains("unknown command"));
        assert!(run(&b, "connect", json!({ "provider": "Real-Debrid" })).unwrap_err().contains("token"));
        assert!(run(&b, "connect", json!({ "provider": "Nope", "token": "x" })).unwrap_err().contains("unknown provider"));
        assert!(run(&b, "transfers", Value::Null).unwrap_err().contains("connect a provider"));
        assert!(run(&b, "login_start", json!({ "provider": "TorBox" })).unwrap_err().contains("no device login"));
        assert_eq!(run(&b, "restore", json!({})).unwrap(), Value::Null);
        assert_eq!(run(&b, "restore", json!({ "session": "" })).unwrap(), Value::Null);
        assert!(run(&b, "restore", json!({ "session": "{not json" })).unwrap_err().contains("bad session"));
        assert_eq!(run(&b, "session", Value::Null).unwrap(), Value::Null);
        assert_eq!(run(&b, "disconnect", Value::Null).unwrap(), Value::Null);
    }

    #[test]
    fn session_round_trips_and_omits_empty_oauth() {
        let s = Session { provider: "TorBox".into(), token: "k".into(), oauth: None };
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("oauth").is_none());
        let o = Session {
            provider: "Real-Debrid".into(),
            token: "a".into(),
            oauth: Some(OAuthRefresh { client_id: "c".into(), client_secret: "s".into(), refresh_token: "r".into() }),
        };
        let back: Session = serde_json::from_str(&serde_json::to_string(&o).unwrap()).unwrap();
        assert_eq!(back, o);
    }

    #[test]
    fn transfer_shape_matches_the_desktop_command() {
        let t = RemoteTransfer {
            id: TransferId("42".into()),
            name: "ubuntu.iso".into(),
            status: TransferStatus::Ready,
            progress: 1.0,
            links: vec![RestrictedLink("x".into())],
        };
        let v = tx_json(&t);
        assert_eq!(v["status"], "Ready");
        assert_eq!(v["links"], 1);
        assert_eq!(v["source"], "debrid");
    }
}
