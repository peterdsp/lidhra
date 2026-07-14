//! Lidhra server: the headless / Web-UI mode.
//!
//! Serves the shared web UI (`ui/index.html`) and exposes the debrid + transfer
//! engine over a small JSON API. This is "Lidhra like qbittorrent-nox": run it,
//! open the page, drive it from any browser. The Tauri desktop app wraps the same
//! UI and calls the crates directly instead of HTTP.
//!
//! Run:  cargo run -p lidhra-server   (then open http://127.0.0.1:8787)
//! Env:  PORT (default 8787), LIDHRA_OUT (download dir, default ./downloads)

use axum::{
    extract::State,
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use lidhra_debrid::prelude::*;
use lidhra_transfer::{download, DownloadConfig, Progress};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::result::Result; // shadow the prelude's `Result` alias back to std's
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Online activation endpoint (the Cloudflare Worker). Override with
/// LIDHRA_ACTIVATE_URL. The app posts {email, machine_id}; the Worker checks the
/// Ko-fi purchase and returns a node-locked key.
const DEFAULT_ACTIVATE_URL: &str = "https://lidhra-license.petros.workers.dev/activate";

const INDEX: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/index.html"));

/// One local file download, shared with its background task via atomics.
struct Dl {
    id: String,
    name: String,
    downloaded: AtomicU64,
    total: AtomicU64, // 0 = unknown
    done: AtomicBool,
    error: std::sync::Mutex<Option<String>>,
}

struct AppState {
    provider: Option<Box<dyn DebridProvider>>,
    out_dir: PathBuf,
    downloads: Vec<Arc<Dl>>,
    next_id: u64,
    config_dir: PathBuf,
    pubkey: String,
}
type Shared = Arc<Mutex<AppState>>;

#[tokio::main]
async fn main() {
    let out_dir = std::env::var("LIDHRA_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join("downloads"));
    let config_dir = std::env::var("LIDHRA_CONFIG").map(PathBuf::from).unwrap_or_else(|_| out_dir.clone());
    let pubkey = std::env::var("LIDHRA_PUBKEY").unwrap_or_else(|_| lidhra_license::ISSUER_PUBKEY_HEX.to_string());
    lidhra_license::load_or_init_install(&config_dir, now_unix());
    let state: Shared = Arc::new(Mutex::new(AppState {
        provider: None,
        out_dir: out_dir.clone(),
        downloads: Vec::new(),
        next_id: 0,
        config_dir,
        pubkey,
    }));

    let app = Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/api/providers", get(providers))
        .route("/api/connect", post(connect))
        .route("/api/add", post(add))
        .route("/api/fetch", post(fetch_url))
        .route("/api/transfers", get(transfers))
        .route("/api/download", post(download_transfer))
        .route("/api/downloads", get(downloads))
        .route("/api/license", get(license).post(activate))
        .route("/api/license/email", post(activate_email))
        .with_state(state);

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8787);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    println!("Lidhra server on http://{addr}   (downloads: {})", out_dir.display());
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
    let n: String = n.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { '_' } else { c }).collect();
    if n.is_empty() { "download".into() } else { n }
}

fn name_from_url(url: &str) -> String {
    let tail = url.rsplit('/').next().unwrap_or("download");
    sanitize(tail.split('?').next().unwrap_or("download"))
}

/// Register a download and spawn its background task. Caller holds the AppState lock.
fn start_download(st: &mut AppState, url: String, name: String) -> Arc<Dl> {
    let dl = Arc::new(Dl {
        id: format!("d{}", st.next_id),
        name,
        downloaded: AtomicU64::new(0),
        total: AtomicU64::new(0),
        done: AtomicBool::new(false),
        error: std::sync::Mutex::new(None),
    });
    st.next_id += 1;
    st.downloads.push(dl.clone());

    let out = st.out_dir.clone();
    let handle = dl.clone();
    tokio::spawn(async move {
        let dest = out.join(&handle.name);
        std::fs::create_dir_all(&out).ok();
        let cb = handle.clone();
        let on_progress = move |p: Progress| {
            cb.downloaded.store(p.downloaded, Ordering::Relaxed);
            if let Some(t) = p.total {
                cb.total.store(t, Ordering::Relaxed);
            }
        };
        match download(&url, &dest, &DownloadConfig::default(), on_progress).await {
            Ok(o) => handle.total.store(o.bytes, Ordering::Relaxed),
            Err(e) => *handle.error.lock().unwrap() = Some(e.to_string()),
        }
        handle.done.store(true, Ordering::Relaxed);
    });
    dl
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
#[derive(Serialize)]
struct DlDto {
    id: String,
    name: String,
    downloaded: u64,
    total: u64,
    progress: f32,
    done: bool,
    error: Option<String>,
}
fn to_dl(d: &Dl) -> DlDto {
    let total = d.total.load(Ordering::Relaxed);
    let downloaded = d.downloaded.load(Ordering::Relaxed);
    DlDto {
        id: d.id.clone(),
        name: d.name.clone(),
        downloaded,
        total,
        progress: if total > 0 { (downloaded as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 },
        done: d.done.load(Ordering::Relaxed),
        error: d.error.lock().unwrap().clone(),
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
    let id = ProviderId::from_key(&req.provider).ok_or_else(|| err(StatusCode::BAD_REQUEST, "unknown provider"))?;
    let p = build_provider(id, Credential::ApiKey(req.token.clone())).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    p.authenticate(Credential::ApiKey(req.token)).await.map_err(|e| err(StatusCode::UNAUTHORIZED, e))?;
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

#[derive(Deserialize)]
struct FetchReq {
    url: String,
}
/// Download any direct http(s) link (no provider needed) - like qBittorrent's "add URL".
async fn fetch_url(State(s): State<Shared>, Json(req): Json<FetchReq>) -> Result<Json<DlDto>, ApiErr> {
    if !(req.url.starts_with("http://") || req.url.starts_with("https://")) {
        return Err(err(StatusCode::BAD_REQUEST, "not an http(s) URL"));
    }
    let name = name_from_url(&req.url);
    let mut st = s.lock().await;
    let dl = start_download(&mut st, req.url, name);
    Ok(Json(to_dl(&dl)))
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
/// Resolve a debrid transfer's files to direct links and start downloading them.
async fn download_transfer(State(s): State<Shared>, Json(req): Json<DlReq>) -> Result<Json<DlResp>, ApiErr> {
    let mut st = s.lock().await;
    let links = {
        let p = st.provider.as_ref().ok_or_else(|| err(StatusCode::BAD_REQUEST, "connect a provider first"))?;
        let t = p.transfer(&TransferId(req.id.clone())).await.map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
        let mut direct = Vec::new();
        for l in &t.links {
            if let Ok(d) = p.unrestrict(l).await {
                direct.push(d);
            }
        }
        direct
    };
    let n = links.len();
    for d in links {
        let name = sanitize(&d.filename);
        start_download(&mut st, d.url, name);
    }
    Ok(Json(DlResp { started: n }))
}

async fn downloads(State(s): State<Shared>) -> Json<Vec<DlDto>> {
    let st = s.lock().await;
    Json(st.downloads.iter().map(|d| to_dl(d)).collect())
}

#[derive(Serialize)]
struct Lic {
    state: String,
    days_left: u32,
    owner: Option<String>,
}
fn lic_dto(st: &AppState) -> Lic {
    let install = lidhra_license::load_or_init_install(&st.config_dir, now_unix());
    let license = lidhra_license::load_license(&st.config_dir);
    match lidhra_license::status(now_unix(), install, license.as_deref(), &st.pubkey) {
        lidhra_license::Status::Licensed { owner } => Lic { state: "licensed".into(), days_left: 0, owner: Some(owner) },
        lidhra_license::Status::Trial { days_left } => Lic { state: "trial".into(), days_left, owner: None },
        lidhra_license::Status::Expired => Lic { state: "expired".into(), days_left: 0, owner: None },
    }
}
async fn license(State(s): State<Shared>) -> Json<Lic> {
    Json(lic_dto(&*s.lock().await))
}

#[derive(Deserialize)]
struct ActReq {
    key: String,
}
async fn activate(State(s): State<Shared>, Json(req): Json<ActReq>) -> Result<Json<Lic>, ApiErr> {
    let st = s.lock().await;
    lidhra_license::activate(&st.config_dir, &req.key, &st.pubkey).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(lic_dto(&st)))
}

#[derive(Deserialize)]
struct EmailReq {
    email: String,
}
/// Online activation: hand the buyer's Ko-fi email to the licence Worker, which
/// verifies the purchase and returns a node-locked key we then activate locally.
async fn activate_email(State(s): State<Shared>, Json(req): Json<EmailReq>) -> Result<Json<Lic>, ApiErr> {
    let (config_dir, pubkey) = {
        let st = s.lock().await;
        (st.config_dir.clone(), st.pubkey.clone())
    };
    let machine = lidhra_license::machine_id(&config_dir);
    let url = std::env::var("LIDHRA_ACTIVATE_URL").unwrap_or_else(|_| DEFAULT_ACTIVATE_URL.to_string());
    let body = serde_json::json!({ "email": req.email.trim(), "machine_id": machine });
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("activation server unreachable: {e}")))?;
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("bad activation response: {e}")))?;
    match v.get("key").and_then(|k| k.as_str()) {
        Some(key) => {
            lidhra_license::activate(&config_dir, key, &pubkey).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
            let st = s.lock().await;
            Ok(Json(lic_dto(&st)))
        }
        None => {
            let msg = v.get("error").and_then(|e| e.as_str()).unwrap_or("activation failed");
            Err(err(StatusCode::BAD_REQUEST, msg))
        }
    }
}
