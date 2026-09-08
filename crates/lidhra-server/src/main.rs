//! Lidhra server: the headless / Web-UI mode.
//!
//! Serves the shared web UI (`ui/index.html`) and exposes the debrid + transfer
//! engine over a small JSON API. This is "Lidhra like qbittorrent-nox": run it,
//! open the page, drive it from any browser. The Tauri desktop app wraps the same
//! UI and calls the crates directly instead of HTTP.
//!
//! Run:  cargo run -p lidhra-server   (then open http://127.0.0.1:8787)
//! Env:  PORT (default 8787), LIDHRA_OUT (download dir, default ./downloads),
//!       LIDHRA_CONFIG (license + engine state, default ~/.config/lidhra/server),
//!       LIDHRA_P2P=0 to run without the on-device torrent engine,
//!       LIDHRA_ALLOW_ANY_HOST=1 to skip the loopback Host header check (reverse proxies)

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{Html, Response},
    routing::{get, post},
    Json, Router,
};
use lidhra_debrid::prelude::*;
use lidhra_transfer::{download, DownloadConfig, Progress};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::result::Result; // shadow the prelude's `Result` alias back to std's
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use axum::response::IntoResponse;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

#[cfg(feature = "p2p")]
use lidhra_torrent::{Engine as TorrentEngine, EngineConfig as TorrentConfig, State as TorrentState, Torrent};

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
    /// On-device torrents for magnets added without a provider.
    #[cfg(feature = "p2p")]
    torrent: Option<Arc<TorrentEngine>>,
}
type Shared = Arc<Mutex<AppState>>;

#[tokio::main]
async fn main() {
    let out_dir = std::env::var("LIDHRA_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join("downloads"));
    // Never inside the download folder: peers pick file names there, and the
    // torrent engine keeps its session file under this directory.
    let config_dir = std::env::var("LIDHRA_CONFIG").map(PathBuf::from).unwrap_or_else(|_| {
        std::env::var("HOME")
            .map(|h| PathBuf::from(h).join(".config").join("lidhra").join("server"))
            .unwrap_or_else(|_| out_dir.join(".lidhra-server"))
    });
    let _ = std::fs::create_dir_all(&config_dir);
    let pubkey = std::env::var("LIDHRA_PUBKEY").unwrap_or_else(|_| lidhra_license::ISSUER_PUBKEY_HEX.to_string());
    lidhra_license::load_or_init_install(&config_dir, now_unix());
    #[cfg(feature = "p2p")]
    let torrent = if std::env::var("LIDHRA_P2P").map(|v| v == "0").unwrap_or(false) {
        None
    } else {
        match TorrentEngine::start(TorrentConfig::new(out_dir.clone(), config_dir.join("torrents"))).await {
            Ok(e) => Some(e),
            Err(e) => {
                eprintln!("lidhra: direct downloads unavailable: {e}");
                None
            }
        }
    };
    let state: Shared = Arc::new(Mutex::new(AppState {
        provider: None,
        out_dir: out_dir.clone(),
        downloads: Vec::new(),
        next_id: 0,
        config_dir,
        pubkey,
        #[cfg(feature = "p2p")]
        torrent,
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
        .route("/api/torrent/files", post(torrent_files))
        .route("/api/torrent/pause", post(torrent_pause))
        .route("/api/torrent/resume", post(torrent_resume))
        .route("/api/torrent/remove", post(torrent_remove))
        .route("/api/p2p", get(p2p_settings).post(p2p_set_settings))
        .layer(middleware::from_fn(require_loopback_host))
        .with_state(state);

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8787);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    println!("Lidhra server on http://{addr}   (downloads: {})", out_dir.display());
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}

// ---------- helpers ----------

/// The server only listens on loopback, so a legitimate request always carries a
/// loopback `Host`. Rejecting anything else defeats DNS rebinding, where a page on
/// `attacker.example` (resolving to 127.0.0.1) drives the API same-origin.
async fn require_loopback_host(req: Request, next: Next) -> Response {
    if std::env::var("LIDHRA_ALLOW_ANY_HOST").map(|v| v == "1").unwrap_or(false) {
        return next.run(req).await;
    }
    let host = req.headers().get("host").and_then(|h| h.to_str().ok()).unwrap_or("");
    if host_is_loopback(host) {
        next.run(req).await
    } else {
        (StatusCode::FORBIDDEN, "Lidhra only answers requests addressed to localhost").into_response()
    }
}

/// `Host` header values that name this machine's loopback interface, with or without a port.
fn host_is_loopback(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host.split(':').next().unwrap_or(""),
    };
    matches!(name.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1")
}

#[cfg(test)]
mod tests {
    use super::host_is_loopback;

    #[test]
    fn loopback_hosts_pass() {
        for h in ["127.0.0.1:8787", "127.0.0.1", "localhost:8787", "LOCALHOST", "[::1]:8787", "[::1]"] {
            assert!(host_is_loopback(h), "{h}");
        }
    }

    #[test]
    fn rebinding_hosts_fail() {
        for h in ["attacker.example:8787", "127.0.0.1.attacker.example", "192.168.1.5:8787", "", "localhost.evil"] {
            assert!(!host_is_loopback(h), "{h}");
        }
    }
}

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
/// One transfer: a debrid cloud transfer (`source: "debrid"`) or a torrent
/// running on this machine (`source: "local"`). P2P fields are zero for debrid.
#[derive(Serialize)]
struct Tx {
    id: String,
    name: String,
    status: String,
    progress: f32,
    links: usize,
    source: &'static str,
    downloaded: u64,
    total: u64,
    down_bps: u64,
    up_bps: u64,
    peers: u32,
    known_peers: u32,
    eta: Option<u64>,
    error: Option<String>,
    pause_reason: Option<String>,
    path: String,
    magnet: Option<String>,
}
fn to_tx(t: &RemoteTransfer) -> Tx {
    Tx {
        id: t.id.0.clone(),
        name: t.name.clone(),
        status: format!("{:?}", t.status),
        progress: t.progress,
        links: t.links.len(),
        source: "debrid",
        downloaded: 0,
        total: 0,
        down_bps: 0,
        up_bps: 0,
        peers: 0,
        known_peers: 0,
        eta: None,
        error: None,
        pause_reason: None,
        path: String::new(),
        magnet: None,
    }
}
#[cfg(feature = "p2p")]
fn torrent_tx(t: &Torrent) -> Tx {
    let status = match t.state {
        TorrentState::Resolving => "Resolving",
        TorrentState::Checking => "Checking",
        TorrentState::Downloading => "Downloading",
        TorrentState::Seeding => "Seeding",
        TorrentState::Paused => "Paused",
        TorrentState::Done => "Done",
        TorrentState::Error => "Error",
    };
    Tx {
        id: t.id.clone(),
        name: t.name.clone(),
        status: status.to_string(),
        progress: t.progress,
        links: t.files,
        source: "local",
        downloaded: t.downloaded,
        total: t.total,
        down_bps: t.down_bps,
        up_bps: t.up_bps,
        peers: t.peers,
        known_peers: t.known_peers,
        eta: t.eta_secs,
        error: t.error.clone(),
        pause_reason: t.pause_reason.map(|r| format!("{r:?}").to_lowercase()),
        path: t.output_folder.to_string_lossy().into_owned(),
        magnet: Some(t.magnet.clone()),
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
    /// Download on this machine even though a provider is connected.
    #[serde(default)]
    direct: bool,
}
/// Add a magnet: to the debrid cloud when a provider is connected (unless
/// `direct`), otherwise to the on-device torrent engine.
async fn add(State(s): State<Shared>, Json(req): Json<AddReq>) -> Result<Json<Tx>, ApiErr> {
    let st = s.lock().await;
    if st.provider.is_some() && !req.direct {
        let m = Magnet::parse(&req.magnet).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
        let p = st.provider.as_ref().ok_or_else(|| err(StatusCode::BAD_REQUEST, "connect a provider first"))?;
        let t = p.add_magnet(&m).await.map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
        return Ok(Json(to_tx(&t)));
    }
    #[cfg(feature = "p2p")]
    {
        let why = if st.provider.is_some() { "direct downloads are not available" } else { "connect a provider first" };
        let e = st.torrent.clone().ok_or_else(|| err(StatusCode::BAD_REQUEST, why))?;
        drop(st);
        let t = e.add_magnet(&req.magnet).await.map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
        Ok(Json(torrent_tx(&t)))
    }
    #[cfg(not(feature = "p2p"))]
    {
        drop(st);
        Err(err(StatusCode::BAD_REQUEST, "connect a provider first"))
    }
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

/// The debrid list (when connected) followed by the torrents on this machine.
async fn transfers(State(s): State<Shared>) -> Result<Json<Vec<Tx>>, ApiErr> {
    let st = s.lock().await;
    let mut out = Vec::new();
    if let Some(p) = st.provider.as_ref() {
        let list = p.list_transfers().await.map_err(|e| err(StatusCode::BAD_GATEWAY, e))?;
        out.extend(list.iter().map(to_tx));
    }
    #[cfg(feature = "p2p")]
    {
        let e = st.torrent.clone();
        drop(st);
        if let Some(e) = e {
            out.extend(e.list().await.iter().map(torrent_tx));
        }
    }
    Ok(Json(out))
}

// ---------- on-device torrents ----------

#[derive(Deserialize)]
struct TorrentReq {
    id: String,
    #[serde(default)]
    delete_files: bool,
}
#[derive(Serialize)]
struct TFile {
    index: usize,
    filename: String,
    size: u64,
    downloaded: u64,
    path: String,
    done: bool,
    included: bool,
}
#[derive(Serialize)]
struct P2pDto {
    available: bool,
    seed: bool,
    wifi_only: bool,
    max_connections: usize,
    mobile: bool,
}
#[derive(Deserialize)]
struct P2pReq {
    seed: bool,
    wifi_only: bool,
    max_connections: usize,
}

#[cfg(feature = "p2p")]
async fn engine(s: &Shared) -> Result<Arc<TorrentEngine>, ApiErr> {
    s.lock().await.torrent.clone().ok_or_else(|| err(StatusCode::BAD_REQUEST, "direct downloads are not available"))
}
#[cfg(feature = "p2p")]
async fn torrent_files(State(s): State<Shared>, Json(req): Json<TorrentReq>) -> Result<Json<Vec<TFile>>, ApiErr> {
    let files = engine(&s).await?.files(&req.id).await.map_err(|e| err(StatusCode::NOT_FOUND, e))?;
    Ok(Json(
        files
            .into_iter()
            .map(|f| TFile {
                index: f.index,
                filename: f.name,
                size: f.size,
                downloaded: f.downloaded,
                path: f.path.to_string_lossy().into_owned(),
                done: f.done,
                included: f.included,
            })
            .collect(),
    ))
}
#[cfg(feature = "p2p")]
async fn torrent_pause(State(s): State<Shared>, Json(req): Json<TorrentReq>) -> Result<Json<()>, ApiErr> {
    engine(&s).await?.pause(&req.id).await.map_err(|e| err(StatusCode::NOT_FOUND, e))?;
    Ok(Json(()))
}
#[cfg(feature = "p2p")]
async fn torrent_resume(State(s): State<Shared>, Json(req): Json<TorrentReq>) -> Result<Json<()>, ApiErr> {
    engine(&s).await?.resume(&req.id).await.map_err(|e| err(StatusCode::NOT_FOUND, e))?;
    Ok(Json(()))
}
#[cfg(feature = "p2p")]
async fn torrent_remove(State(s): State<Shared>, Json(req): Json<TorrentReq>) -> Result<Json<()>, ApiErr> {
    engine(&s).await?.remove(&req.id, req.delete_files).await.map_err(|e| err(StatusCode::NOT_FOUND, e))?;
    Ok(Json(()))
}
#[cfg(feature = "p2p")]
async fn p2p_settings(State(s): State<Shared>) -> Json<P2pDto> {
    let e = s.lock().await.torrent.clone();
    let Some(e) = e else {
        return Json(P2pDto { available: false, seed: false, wifi_only: true, max_connections: 50, mobile: false });
    };
    let st = e.settings().await;
    Json(P2pDto { available: true, seed: st.seed, wifi_only: st.wifi_only, max_connections: st.max_connections, mobile: false })
}
#[cfg(feature = "p2p")]
async fn p2p_set_settings(State(s): State<Shared>, Json(req): Json<P2pReq>) -> Result<Json<P2pDto>, ApiErr> {
    let e = engine(&s).await?;
    let settings = lidhra_torrent::Settings {
        seed: req.seed,
        wifi_only: req.wifi_only,
        max_connections: req.max_connections.clamp(2, 200),
    };
    e.set_settings(settings).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(p2p_settings(State(s)).await)
}

#[cfg(not(feature = "p2p"))]
const NO_P2P: &str = "direct downloads are not included in this build";
#[cfg(not(feature = "p2p"))]
async fn torrent_files(State(_s): State<Shared>, Json(_r): Json<TorrentReq>) -> Result<Json<Vec<TFile>>, ApiErr> {
    Err(err(StatusCode::NOT_IMPLEMENTED, NO_P2P))
}
#[cfg(not(feature = "p2p"))]
async fn torrent_pause(State(_s): State<Shared>, Json(_r): Json<TorrentReq>) -> Result<Json<()>, ApiErr> {
    Err(err(StatusCode::NOT_IMPLEMENTED, NO_P2P))
}
#[cfg(not(feature = "p2p"))]
async fn torrent_resume(State(_s): State<Shared>, Json(_r): Json<TorrentReq>) -> Result<Json<()>, ApiErr> {
    Err(err(StatusCode::NOT_IMPLEMENTED, NO_P2P))
}
#[cfg(not(feature = "p2p"))]
async fn torrent_remove(State(_s): State<Shared>, Json(_r): Json<TorrentReq>) -> Result<Json<()>, ApiErr> {
    Err(err(StatusCode::NOT_IMPLEMENTED, NO_P2P))
}
#[cfg(not(feature = "p2p"))]
async fn p2p_settings(State(_s): State<Shared>) -> Json<P2pDto> {
    Json(P2pDto { available: false, seed: false, wifi_only: true, max_connections: 50, mobile: false })
}
#[cfg(not(feature = "p2p"))]
async fn p2p_set_settings(State(_s): State<Shared>, Json(_r): Json<P2pReq>) -> Result<Json<P2pDto>, ApiErr> {
    Err(err(StatusCode::NOT_IMPLEMENTED, NO_P2P))
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
