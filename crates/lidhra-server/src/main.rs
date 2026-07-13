//! Lidhra server — the headless / Web-UI mode.
//!
//! Serves the shared web UI (`ui/index.html`) and exposes the debrid + transfer
//! engine over a small JSON API. This is "Lidhra like qbittorrent-nox": run it,
//! open the page, drive it from any browser. The Tauri desktop app and the smart-TV
//! clients wrap the *same* UI; on desktop they call the crates directly instead of HTTP.
//!
//! Run:  cargo run -p lidhra-server   (then open http://127.0.0.1:8787)
//! Env:  PORT (default 8787) · LIDHRA_OUT (download dir, default ./downloads)

use axum::{
    extract::State,
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use lidhra_debrid::prelude::*;
use lidhra_transfer::{download, DownloadConfig};
use std::result::Result; // shadow the prelude's `Result` alias back to std's
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

const INDEX: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/index.html"));

struct AppState {
    provider: Option<Box<dyn DebridProvider>>,
    out_dir: PathBuf,
}
type Shared = Arc<Mutex<AppState>>;

#[tokio::main]
async fn main() {
    let out_dir = std::env::var("LIDHRA_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join("downloads"));
    let state: Shared = Arc::new(Mutex::new(AppState { provider: None, out_dir: out_dir.clone() }));

    let app = Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/api/providers", get(providers))
        .route("/api/connect", post(connect))
        .route("/api/add", post(add))
        .route("/api/transfers", get(transfers))
        .route("/api/download", post(download_h))
        .with_state(state);

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8787);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    println!("Lidhra server → http://{addr}   (downloads: {})", out_dir.display());
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}

// ---------- helpers ----------

type ApiErr = (StatusCode, Json<serde_json::Value>);
fn err(code: StatusCode, msg: impl ToString) -> ApiErr {
    (code, Json(serde_json::json!({ "error": msg.to_string() })))
}

fn sanitize(name: &str) -> String {
    let n = Path::new(name).file_name().and_then(|x| x.to_str()).unwrap_or(name);
    n.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { '_' } else { c }).collect()
}

// ---------- DTOs ----------

#[derive(Serialize)]
struct Prov {
    id: String,
    label: String,
}

#[derive(Serialize)]
struct Acct {
    username: String,
    premium: bool,
}

#[derive(Serialize)]
struct Tx {
    id: String,
    name: String,
    status: String,
    progress: f32,
    links: usize,
}
fn to_tx(t: &RemoteTransfer) -> Tx {
    Tx {
        id: t.id.0.clone(),
        name: t.name.clone(),
        status: format!("{:?}", t.status),
        progress: t.progress,
        links: t.links.len(),
    }
}

// ---------- handlers ----------

async fn providers() -> Json<Vec<Prov>> {
    Json(
        ProviderId::IMPLEMENTED
            .iter()
            .map(|p| Prov { id: p.label().to_string(), label: p.label().to_string() })
            .collect(),
    )
}

#[derive(Deserialize)]
struct ConnectReq {
    provider: String,
    token: String,
}
async fn connect(State(s): State<Shared>, Json(req): Json<ConnectReq>) -> Result<Json<Acct>, ApiErr> {
    let id = ProviderId::from_key(&req.provider)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "unknown provider"))?;
    let p = build_provider(id, Credential::ApiKey(req.token.clone()))
        .map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    p.authenticate(Credential::ApiKey(req.token))
        .await
        .map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
    let a = p.account().await.map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    s.lock().await.provider = Some(p);
    Ok(Json(Acct { username: a.username, premium: a.premium }))
}

#[derive(Deserialize)]
struct AddReq {
    magnet: String,
}
async fn add(State(s): State<Shared>, Json(req): Json<AddReq>) -> Result<Json<Tx>, ApiErr> {
    let m = Magnet::parse(&req.magnet).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    let st = s.lock().await;
    let p = st.provider.as_ref().ok_or_else(|| err(StatusCode::BAD_REQUEST, "connect a provider first"))?;
    let t = p.add_magnet(&m).await.map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(to_tx(&t)))
}

async fn transfers(State(s): State<Shared>) -> Result<Json<Vec<Tx>>, ApiErr> {
    let st = s.lock().await;
    let p = st.provider.as_ref().ok_or_else(|| err(StatusCode::BAD_REQUEST, "connect a provider first"))?;
    let list = p.list_transfers().await.map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(list.iter().map(to_tx).collect()))
}

#[derive(Deserialize)]
struct DlReq {
    id: String,
}
#[derive(Serialize)]
struct DlResp {
    started: usize,
}
async fn download_h(State(s): State<Shared>, Json(req): Json<DlReq>) -> Result<Json<DlResp>, ApiErr> {
    // Resolve the transfer's links to direct URLs (needs the provider), then
    // download each in the background.
    let (links, out) = {
        let st = s.lock().await;
        let p = st.provider.as_ref().ok_or_else(|| err(StatusCode::BAD_REQUEST, "connect a provider first"))?;
        let t = p.transfer(&TransferId(req.id.clone())).await.map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
        let mut direct = Vec::new();
        for l in &t.links {
            if let Ok(d) = p.unrestrict(l).await {
                direct.push(d);
            }
        }
        (direct, st.out_dir.clone())
    };
    let n = links.len();
    tokio::fs::create_dir_all(&out).await.ok();
    for d in links {
        let out = out.clone();
        tokio::spawn(async move {
            let dest = out.join(sanitize(&d.filename));
            let _ = download(&d.url, &dest, &DownloadConfig::default(), |_p| {}).await;
        });
    }
    Ok(Json(DlResp { started: n }))
}
