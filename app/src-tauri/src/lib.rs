//! Lidhra desktop (Tauri): native shell over the shared web UI (`ui/`).
//!
//! The same `ui/index.html` the server serves runs here too; when it detects a
//! Tauri window it calls these `#[tauri::command]`s via `invoke` instead of HTTP.
//! Commands drive the `lidhra-debrid` + `lidhra-transfer` crates directly, and,
//! with the `p2p` feature, the on-device `lidhra-torrent` engine for magnets
//! added without a debrid account.

use lidhra_debrid::prelude::*;
use lidhra_transfer::{download as fetch_file, DownloadConfig, Progress};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::result::Result; // shadow the prelude's `Result` alias back to std's
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri_plugin_lidhra_native::NativeExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

#[cfg(feature = "p2p")]
use lidhra_torrent::{Engine as TorrentEngine, EngineConfig as TorrentConfig, State as TorrentState, Torrent};

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// One local file download, shared with its background task via atomics.
struct Dl {
    id: String,
    name: String,
    /// Source URL (kept so the UI can stream / copy it while it downloads).
    url: String,
    /// Final on-disk location (the engine writes `<path>.part` until done).
    path: PathBuf,
    downloaded: AtomicU64,
    total: AtomicU64,
    done: AtomicBool,
    error: std::sync::Mutex<Option<String>>,
}

#[derive(Default)]
struct Inner {
    provider: Option<Box<dyn DebridProvider>>,
    out_dir: Option<PathBuf>,
    downloads: Vec<Arc<Dl>>,
    next_id: u64,
    config_dir: PathBuf,
    /// Where the provider login is remembered (inside the app's own data
    /// directory, so it survives restarts and app updates). Set in `setup`.
    session_path: Option<PathBuf>,
    /// The on-device BitTorrent engine. `None` until it has started (a second
    /// or so after launch) or when the build has no `p2p` feature.
    #[cfg(feature = "p2p")]
    torrent: Option<Arc<TorrentEngine>>,
}
struct AppState(Mutex<Inner>);

/// The remembered provider login.
#[derive(Serialize, Deserialize)]
struct Session {
    provider: String,
    token: String,
}

fn load_session(path: Option<&Path>) -> Option<Session> {
    let bytes = std::fs::read(path?).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_session(path: Option<&Path>, s: &Session) {
    let Some(path) = path else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec(s) {
        let _ = std::fs::write(path, json);
        // Owner-only on Unix; on iOS the container is additionally encrypted at rest.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

#[derive(Serialize)]
struct Prov {
    id: String,
    label: String,
}
#[derive(Serialize)]
struct Acct {
    username: String,
    premium: bool,
    /// The provider key the account was connected with (so the UI can preselect it).
    provider: String,
}

/// One transfer in the list: a debrid cloud transfer (`source: "debrid"`) or a
/// torrent running on this device (`source: "local"`). The P2P fields are zero
/// for debrid transfers.
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
    path: String,
    url: String,
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
        path: d.path.to_string_lossy().into_owned(),
        url: d.url.clone(),
    }
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

/// Register a download and spawn its background task. Caller holds the state lock.
fn start_download(inner: &mut Inner, url: String, name: String) -> Arc<Dl> {
    let out = inner.out_dir.clone().unwrap_or_else(|| PathBuf::from("."));
    let dl = Arc::new(Dl {
        id: format!("d{}", inner.next_id),
        path: out.join(&name),
        name,
        url: url.clone(),
        downloaded: AtomicU64::new(0),
        total: AtomicU64::new(0),
        done: AtomicBool::new(false),
        error: std::sync::Mutex::new(None),
    });
    inner.next_id += 1;
    inner.downloads.push(dl.clone());
    let handle = dl.clone();
    tauri::async_runtime::spawn(async move {
        let dest = handle.path.clone();
        std::fs::create_dir_all(&out).ok();
        let cb = handle.clone();
        let on_progress = move |p: Progress| {
            cb.downloaded.store(p.downloaded, Ordering::Relaxed);
            if let Some(t) = p.total {
                cb.total.store(t, Ordering::Relaxed);
            }
        };
        match fetch_file(&url, &dest, &DownloadConfig::default(), on_progress).await {
            Ok(o) => handle.total.store(o.bytes, Ordering::Relaxed),
            Err(e) => *handle.error.lock().unwrap() = Some(e.to_string()),
        }
        handle.done.store(true, Ordering::Relaxed);
    });
    dl
}

#[tauri::command]
fn providers() -> Vec<Prov> {
    ProviderId::IMPLEMENTED
        .iter()
        .map(|p| Prov { id: p.label().to_string(), label: p.label().to_string() })
        .collect()
}

/// Authenticate against a provider. Runs without the state lock so polling
/// keeps going while the network round-trips happen.
async fn sign_in(provider: &str, token: &str) -> Result<(Box<dyn DebridProvider>, Acct), String> {
    let id = ProviderId::from_key(provider).ok_or("unknown provider")?;
    let p = build_provider(id, Credential::ApiKey(token.to_string())).map_err(|e| e.to_string())?;
    p.authenticate(Credential::ApiKey(token.to_string())).await.map_err(|e| e.to_string())?;
    let a = p.account().await.map_err(|e| e.to_string())?;
    Ok((p, Acct { username: a.username, premium: a.premium, provider: provider.to_string() }))
}

#[tauri::command]
async fn connect(state: tauri::State<'_, AppState>, provider: String, token: String) -> Result<Acct, String> {
    let (p, acct) = sign_in(&provider, &token).await?;
    let mut g = state.0.lock().await;
    g.provider = Some(p);
    save_session(g.session_path.as_deref(), &Session { provider, token });
    Ok(acct)
}

/// Sign back in with the remembered login, if any. `Ok(None)` when nothing is
/// saved; an error (offline, revoked token) keeps the saved login so the next
/// launch can try again.
#[tauri::command]
async fn restore(state: tauri::State<'_, AppState>) -> Result<Option<Acct>, String> {
    let path = state.0.lock().await.session_path.clone();
    let Some(s) = load_session(path.as_deref()) else { return Ok(None) };
    let (p, acct) = sign_in(&s.provider, &s.token).await?;
    state.0.lock().await.provider = Some(p);
    Ok(Some(acct))
}

/// Forget the remembered login and drop the active provider.
#[tauri::command]
async fn disconnect(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut g = state.0.lock().await;
    g.provider = None;
    if let Some(p) = g.session_path.as_deref() {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

/// Add a magnet. With a provider connected it goes to the debrid cloud unless
/// `direct` is set; without one it downloads on this device (P2P). The UI never
/// gets switched silently: `direct` is an explicit choice in the add sheet.
#[tauri::command]
async fn add(state: tauri::State<'_, AppState>, magnet: String, direct: Option<bool>) -> Result<Tx, String> {
    let g = state.0.lock().await;
    let use_debrid = g.provider.is_some() && !direct.unwrap_or(false);
    if use_debrid {
        let m = Magnet::parse(&magnet).map_err(|e| e.to_string())?;
        let p = g.provider.as_ref().ok_or("connect a provider first")?;
        let t = p.add_magnet(&m).await.map_err(|e| e.to_string())?;
        return Ok(to_tx(&t));
    }
    #[cfg(feature = "p2p")]
    {
        let engine = g.torrent.clone().ok_or("direct downloads are still starting, try again in a moment")?;
        drop(g);
        let t = engine.add_magnet(&magnet).await.map_err(|e| e.to_string())?;
        Ok(torrent_tx(&t))
    }
    #[cfg(not(feature = "p2p"))]
    {
        drop(g);
        Err("connect a provider first".into())
    }
}

#[tauri::command]
async fn fetch(state: tauri::State<'_, AppState>, url: String) -> Result<DlDto, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("not an http(s) URL".into());
    }
    let name = name_from_url(&url);
    let mut g = state.0.lock().await;
    let dl = start_download(&mut g, url, name);
    Ok(to_dl(&dl))
}

/// Every transfer: the debrid cloud list (when connected) followed by the
/// torrents running on this device. A debrid error is returned as before so
/// the UI keeps its last list instead of blanking it.
#[tauri::command]
async fn transfers(state: tauri::State<'_, AppState>) -> Result<Vec<Tx>, String> {
    let g = state.0.lock().await;
    let mut out = Vec::new();
    if let Some(p) = g.provider.as_ref() {
        let list = p.list_transfers().await.map_err(|e| e.to_string())?;
        out.extend(list.iter().map(to_tx));
    }
    #[cfg(feature = "p2p")]
    {
        let engine = g.torrent.clone();
        drop(g);
        if let Some(e) = engine {
            out.extend(e.list().await.iter().map(torrent_tx));
        }
    }
    Ok(out)
}

#[tauri::command]
async fn download(state: tauri::State<'_, AppState>, id: String) -> Result<usize, String> {
    let mut g = state.0.lock().await;
    let links = {
        let p = g.provider.as_ref().ok_or("connect a provider first")?;
        let t = p.transfer(&TransferId(id)).await.map_err(|e| e.to_string())?;
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
        start_download(&mut g, d.url, name);
    }
    Ok(n)
}

#[tauri::command]
async fn downloads(state: tauri::State<'_, AppState>) -> Result<Vec<DlDto>, String> {
    let g = state.0.lock().await;
    Ok(g.downloads.iter().map(|d| to_dl(d)).collect())
}

/// One file inside a ready transfer, already unrestricted to a direct URL.
#[derive(Serialize)]
struct FileLink {
    url: String,
    filename: String,
    size: u64,
    mime: Option<String>,
}

/// Resolve every file of a transfer to a direct HTTPS link (for the file sheet:
/// stream it, hand it to VLC, download just that one, copy the link).
#[tauri::command]
async fn links(state: tauri::State<'_, AppState>, id: String) -> Result<Vec<FileLink>, String> {
    let g = state.0.lock().await;
    let p = g.provider.as_ref().ok_or("connect a provider first")?;
    let t = p.transfer(&TransferId(id)).await.map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(t.links.len());
    for l in &t.links {
        let d = p.unrestrict(l).await.map_err(|e| e.to_string())?;
        out.push(FileLink { url: d.url, filename: sanitize(&d.filename), size: d.size, mime: d.mime });
    }
    Ok(out)
}

/// Download a single already-unrestricted file (as opposed to `download`,
/// which fetches every file of a transfer).
#[tauri::command]
async fn download_link(state: tauri::State<'_, AppState>, url: String, filename: String) -> Result<DlDto, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("not an http(s) URL".into());
    }
    let name = sanitize(&filename);
    let mut g = state.0.lock().await;
    let dl = start_download(&mut g, url, name);
    Ok(to_dl(&dl))
}

// ---- on-device torrents (the `p2p` feature) ----------------------------------

/// One file of a local torrent, for the file sheet.
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

/// What the UI needs to show the P2P settings block (and whether to show it at all).
#[derive(Serialize)]
struct P2pDto {
    /// False in a build without the `p2p` feature, or until the engine has started.
    available: bool,
    seed: bool,
    wifi_only: bool,
    max_connections: usize,
    /// True on iOS, where the Wi-Fi only toggle and the background limits apply.
    mobile: bool,
}

#[cfg(feature = "p2p")]
async fn engine(state: &tauri::State<'_, AppState>) -> Result<Arc<TorrentEngine>, String> {
    state.0.lock().await.torrent.clone().ok_or_else(|| "direct downloads are not available".to_string())
}

#[cfg(feature = "p2p")]
#[tauri::command]
async fn torrent_files(state: tauri::State<'_, AppState>, id: String) -> Result<Vec<TFile>, String> {
    let e = engine(&state).await?;
    let files = e.files(&id).await.map_err(|e| e.to_string())?;
    Ok(files
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
        .collect())
}

#[cfg(feature = "p2p")]
#[tauri::command]
async fn torrent_pause(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    engine(&state).await?.pause(&id).await.map_err(|e| e.to_string())
}

#[cfg(feature = "p2p")]
#[tauri::command]
async fn torrent_resume(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    engine(&state).await?.resume(&id).await.map_err(|e| e.to_string())
}

#[cfg(feature = "p2p")]
#[tauri::command(rename_all = "snake_case")]
async fn torrent_remove(state: tauri::State<'_, AppState>, id: String, delete_files: Option<bool>) -> Result<(), String> {
    engine(&state).await?.remove(&id, delete_files.unwrap_or(false)).await.map_err(|e| e.to_string())
}

#[cfg(feature = "p2p")]
#[tauri::command]
async fn p2p_settings(state: tauri::State<'_, AppState>) -> Result<P2pDto, String> {
    let Some(e) = state.0.lock().await.torrent.clone() else {
        return Ok(P2pDto { available: false, seed: false, wifi_only: true, max_connections: 50, mobile: cfg!(target_os = "ios") });
    };
    let s = e.settings().await;
    Ok(P2pDto { available: true, seed: s.seed, wifi_only: s.wifi_only, max_connections: s.max_connections, mobile: cfg!(target_os = "ios") })
}

#[cfg(feature = "p2p")]
#[tauri::command(rename_all = "snake_case")]
async fn p2p_set_settings(
    state: tauri::State<'_, AppState>,
    seed: bool,
    wifi_only: bool,
    max_connections: usize,
) -> Result<P2pDto, String> {
    let e = engine(&state).await?;
    let s = lidhra_torrent::Settings { seed, wifi_only, max_connections: max_connections.clamp(2, 200) };
    e.set_settings(s).await.map_err(|e| e.to_string())?;
    p2p_settings(state).await
}

// Builds without the engine keep the command surface so the UI needs no
// feature detection beyond `available: false`.
#[cfg(not(feature = "p2p"))]
#[tauri::command]
async fn torrent_files(_s: tauri::State<'_, AppState>, _id: String) -> Result<Vec<TFile>, String> {
    Err("direct downloads are not included in this build".into())
}
#[cfg(not(feature = "p2p"))]
#[tauri::command]
async fn torrent_pause(_s: tauri::State<'_, AppState>, _id: String) -> Result<(), String> {
    Err("direct downloads are not included in this build".into())
}
#[cfg(not(feature = "p2p"))]
#[tauri::command]
async fn torrent_resume(_s: tauri::State<'_, AppState>, _id: String) -> Result<(), String> {
    Err("direct downloads are not included in this build".into())
}
#[cfg(not(feature = "p2p"))]
#[tauri::command(rename_all = "snake_case")]
async fn torrent_remove(_s: tauri::State<'_, AppState>, _id: String, _delete_files: Option<bool>) -> Result<(), String> {
    Err("direct downloads are not included in this build".into())
}
#[cfg(not(feature = "p2p"))]
#[tauri::command]
async fn p2p_settings(_s: tauri::State<'_, AppState>) -> Result<P2pDto, String> {
    Ok(P2pDto { available: false, seed: false, wifi_only: true, max_connections: 50, mobile: cfg!(target_os = "ios") })
}
#[cfg(not(feature = "p2p"))]
#[tauri::command(rename_all = "snake_case")]
async fn p2p_set_settings(
    _s: tauri::State<'_, AppState>,
    _seed: bool,
    _wifi_only: bool,
    _max_connections: usize,
) -> Result<P2pDto, String> {
    Err("direct downloads are not included in this build".into())
}

// ---- iOS lifecycle bridge -----------------------------------------------------
//
// The Swift plugin calls `lidhra_native_event` (plain C ABI, resolved by the
// linker from this static library) when the app is about to be suspended,
// comes back to the foreground, changes network, or toggles Low Power Mode.
// The call only queues an event; a task on the async runtime applies it to the
// engine. "Suspending" waits (briefly) for the engine to pause and save, since
// the process is frozen right after the call returns.

#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(feature = "p2p"), allow(dead_code))]
enum NativeEvent {
    /// App state: 0 = suspending (background time is up, or terminating),
    /// 1 = foreground, 2 = entered background (grace period, keep going).
    App(i32),
    /// Network: 0 = Wi-Fi or wired, 1 = cellular or otherwise expensive, 2 = offline.
    Network(i32),
    /// Low Power Mode: 0 = off, 1 = on.
    LowPower(i32),
}

struct QueuedEvent {
    event: NativeEvent,
    ack: Option<std::sync::mpsc::SyncSender<()>>,
}

static NATIVE_EVENTS: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<QueuedEvent>> = std::sync::OnceLock::new();

/// Entry point for the Swift side. Safe to call from any thread, before the
/// engine exists (events are dropped until the queue is up).
#[no_mangle]
pub extern "C" fn lidhra_native_event(kind: i32, value: i32) {
    let event = match kind {
        1 => NativeEvent::App(value),
        2 => NativeEvent::Network(value),
        3 => NativeEvent::LowPower(value),
        _ => return,
    };
    let Some(tx) = NATIVE_EVENTS.get() else { return };
    let wait = matches!(event, NativeEvent::App(0));
    let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel::<()>(1);
    if tx.send(QueuedEvent { event, ack: wait.then_some(ack_tx) }).is_err() {
        return;
    }
    if wait {
        // iOS gives an expiration handler a few seconds; keep well inside that.
        let _ = ack_rx.recv_timeout(std::time::Duration::from_millis(2500));
    }
}

fn start_native_event_loop(app: tauri::AppHandle) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<QueuedEvent>();
    if NATIVE_EVENTS.set(tx).is_err() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        while let Some(q) = rx.recv().await {
            #[cfg(feature = "p2p")]
            {
                use tauri::Manager;
                let engine = app.state::<AppState>().0.lock().await.torrent.clone();
                if let Some(e) = engine {
                    match q.event {
                        NativeEvent::App(0) => e.set_suspended(true).await,
                        NativeEvent::App(1) => e.set_suspended(false).await,
                        NativeEvent::App(_) => {}
                        NativeEvent::Network(n) => e.set_cellular(n == 1).await,
                        NativeEvent::LowPower(n) => e.set_low_power(n == 1).await,
                    }
                }
            }
            #[cfg(not(feature = "p2p"))]
            {
                let _ = (&app, q.event);
            }
            if let Some(a) = q.ack {
                let _ = a.try_send(());
            }
        }
    });
}

// ---- file actions (the "tap a file" sheet) ----------------------------------
//
// Desktop: the opener plugin (default app, a named app such as VLC, reveal in
// the file manager). iOS: the native bridge (share sheet, open-in menu,
// AVPlayer). Every command runs on the async runtime, never the main thread,
// because the iOS bridge presents UI on the main thread and waits for it.

fn is_http(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Open a local path or a URL with the default handler, or with a named app
/// (`with: "VLC"`) when the platform supports it.
#[tauri::command]
async fn open_with(app: tauri::AppHandle, target: String, with: Option<String>) -> Result<(), String> {
    let o = app.opener();
    let r = if is_http(&target) { o.open_url(target, with) } else { o.open_path(target, with) };
    r.map_err(|e| e.to_string())
}

/// Reveal a downloaded file in Finder / Explorer / the file manager.
#[tauri::command]
async fn reveal(app: tauri::AppHandle, path: String) -> Result<(), String> {
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}

/// iOS share sheet for a downloaded file (includes "Save to Files"). `anchor`
/// is the initiating control's box in the webview's CSS-pixel space, so the
/// popover points at it instead of a fixed corner (see NativePlugin.swift).
#[tauri::command]
async fn file_share(app: tauri::AppHandle, path: String, anchor: Option<serde_json::Value>) -> Result<(), String> {
    app.native().share(&path, anchor)
}

/// iOS "Open in…" menu for a downloaded file (VLC, Infuse, …).
#[tauri::command]
async fn file_open_in(app: tauri::AppHandle, path: String, anchor: Option<serde_json::Value>) -> Result<(), String> {
    app.native().open_in(&path, anchor)
}

/// iOS native player for a local path or an http(s) URL.
#[tauri::command]
async fn file_play(app: tauri::AppHandle, url: String, title: Option<String>) -> Result<(), String> {
    app.native().play(&url, title.as_deref())
}

/// Whether an app handles this URL scheme on this device (iOS only; false elsewhere).
#[tauri::command]
async fn native_can_open(app: tauri::AppHandle, url: String) -> Result<bool, String> {
    app.native().can_open(&url)
}

#[derive(Serialize)]
struct Lic {
    state: String,
    days_left: u32,
    owner: Option<String>,
    /// True when an external store (Apple) handles all purchasing. The UI must
    /// then never show trial, license-key, or external purchase UI (guideline
    /// 3.1.1). Only the Ko-fi / direct build sets this false.
    store: bool,
}

#[cfg(not(feature = "appstore"))]
fn lic_dto(dir: &Path) -> Lic {
    let install = lidhra_license::load_or_init_install(dir, now_unix());
    let lic = lidhra_license::load_license(dir);
    match lidhra_license::status(now_unix(), install, lic.as_deref(), lidhra_license::ISSUER_PUBKEY_HEX) {
        lidhra_license::Status::Licensed { owner } => Lic { state: "licensed".into(), days_left: 0, owner: Some(owner), store: false },
        lidhra_license::Status::Trial { days_left } => Lic { state: "trial".into(), days_left, owner: None, store: false },
        lidhra_license::Status::Expired => Lic { state: "expired".into(), days_left: 0, owner: None, store: false },
    }
}

// Ko-fi / direct build: real trial + license.
#[cfg(not(feature = "appstore"))]
#[tauri::command]
async fn license(state: tauri::State<'_, AppState>) -> Result<Lic, String> {
    let dir = state.0.lock().await.config_dir.clone();
    Ok(lic_dto(&dir))
}
#[cfg(not(feature = "appstore"))]
#[tauri::command]
async fn license_activate(state: tauri::State<'_, AppState>, key: String) -> Result<Lic, String> {
    let dir = state.0.lock().await.config_dir.clone();
    lidhra_license::activate(&dir, &key, lidhra_license::ISSUER_PUBKEY_HEX)?;
    Ok(lic_dto(&dir))
}
// Online activation: send the buyer's Ko-fi email to the licence Worker, which
// verifies the purchase and returns a node-locked key we activate locally.
#[cfg(not(feature = "appstore"))]
#[tauri::command]
async fn license_activate_email(state: tauri::State<'_, AppState>, email: String) -> Result<Lic, String> {
    let dir = state.0.lock().await.config_dir.clone();
    let machine = lidhra_license::machine_id(&dir);
    let url = std::env::var("LIDHRA_ACTIVATE_URL")
        .unwrap_or_else(|_| "https://lidhra-license.petros.workers.dev/activate".to_string());
    let body = serde_json::json!({ "email": email.trim(), "machine_id": machine });
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("activation server unreachable: {e}"))?;
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("bad activation response: {e}"))?;
    match v.get("key").and_then(|k| k.as_str()) {
        Some(key) => {
            lidhra_license::activate(&dir, key, lidhra_license::ISSUER_PUBKEY_HEX)?;
            Ok(lic_dto(&dir))
        }
        None => Err(v.get("error").and_then(|e| e.as_str()).unwrap_or("activation failed").to_string()),
    }
}

// App Store build: paid upfront, always licensed, no trial. `store: true` tells
// the UI to hide every trial / license-key / external purchase element.
#[cfg(feature = "appstore")]
#[tauri::command]
async fn license(_s: tauri::State<'_, AppState>) -> Result<Lic, String> {
    Ok(Lic { state: "licensed".into(), days_left: 0, owner: Some("App Store".into()), store: true })
}
#[cfg(feature = "appstore")]
#[tauri::command]
async fn license_activate(_s: tauri::State<'_, AppState>, _key: String) -> Result<Lic, String> {
    Ok(Lic { state: "licensed".into(), days_left: 0, owner: Some("App Store".into()), store: true })
}
#[cfg(feature = "appstore")]
#[tauri::command]
async fn license_activate_email(_s: tauri::State<'_, AppState>, _email: String) -> Result<Lic, String> {
    Ok(Lic { state: "licensed".into(), days_left: 0, owner: Some("App Store".into()), store: true })
}

/// Start the on-device torrent engine in the background and hand it to the
/// app state once it is up. Files land next to the HTTPS downloads; the
/// session file, settings and DHT table live in the app's data directory.
#[cfg(feature = "p2p")]
fn start_torrent_engine(app: &tauri::AppHandle, out_dir: Option<PathBuf>, data_dir: PathBuf) {
    use tauri::Manager;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let out = out_dir.unwrap_or_else(|| PathBuf::from("."));
        let cfg = TorrentConfig::new(out, data_dir.join("torrents"));
        match TorrentEngine::start(cfg).await {
            Ok(e) => handle.state::<AppState>().0.lock().await.torrent = Some(e),
            Err(err) => eprintln!("lidhra: direct downloads unavailable: {err}"),
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // reqwest 0.13 (Tauri's mobile dev-server proxy, the updater) refuses to
    // build a client until a rustls crypto provider is installed process-wide;
    // without this the app aborts at launch under `tauri ios dev`. Idempotent.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // iOS: the app's Documents folder, which Info.ios.plist exposes in the
    // Files app ("On My iPhone > Lidhra"). Elsewhere: ~/Downloads.
    let downloads_folder = if cfg!(target_os = "ios") { "Documents" } else { "Downloads" };
    let out_dir = std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(downloads_folder));
    let config_dir = std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config").join("lidhra"))
        .unwrap_or_else(|| PathBuf::from(".lidhra"));

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_lidhra_native::init())
        .manage(AppState(Mutex::new(Inner { out_dir, config_dir, ..Default::default() })))
        .setup(|app| {
            use tauri::Manager;
            // Remember the provider login in the app's data directory.
            let data_dir = app.path().app_data_dir().ok();
            if let Some(dir) = &data_dir {
                let state = app.state::<AppState>();
                state.0.blocking_lock().session_path = Some(dir.join("session.json"));
            }

            start_native_event_loop(app.handle().clone());

            #[cfg(feature = "p2p")]
            {
                let out = app.state::<AppState>().0.blocking_lock().out_dir.clone();
                let data = data_dir.clone().unwrap_or_else(|| PathBuf::from(".lidhra"));
                start_torrent_engine(app.handle(), out, data);
            }

            // Auto-update only in the Ko-fi / direct build; the App Store handles updates itself.
            #[cfg(not(feature = "appstore"))]
            {
                use tauri_plugin_updater::UpdaterExt;
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(updater) = handle.updater() {
                        if let Ok(Some(update)) = updater.check().await {
                            let _ = update.download_and_install(|_, _| {}, || {}).await;
                        }
                    }
                });
            }
            Ok(())
        });

    #[cfg(not(feature = "appstore"))]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        .invoke_handler(tauri::generate_handler![
            providers, connect, restore, disconnect, add, fetch, transfers, download, downloads, links, download_link, open_with,
            reveal, file_share, file_open_in, file_play, native_can_open, license, license_activate,
            license_activate_email, torrent_files, torrent_pause, torrent_resume, torrent_remove, p2p_settings,
            p2p_set_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lidhra");
}
