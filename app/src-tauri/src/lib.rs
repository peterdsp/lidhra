//! Lidhra desktop (Tauri): native shell over the shared web UI (`ui/`).
//!
//! The same `ui/index.html` the server serves runs here too; when it detects a
//! Tauri window it calls these `#[tauri::command]`s via `invoke` instead of HTTP.
//! Commands drive the `lidhra-debrid` + `lidhra-transfer` crates directly.

use lidhra_debrid::prelude::*;
use lidhra_transfer::{download as fetch_file, DownloadConfig, Progress};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::result::Result; // shadow the prelude's `Result` alias back to std's
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri_plugin_lidhra_native::NativeExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

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
}
struct AppState(Mutex<Inner>);

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

#[tauri::command]
async fn connect(state: tauri::State<'_, AppState>, provider: String, token: String) -> Result<Acct, String> {
    let id = ProviderId::from_key(&provider).ok_or("unknown provider")?;
    let p = build_provider(id, Credential::ApiKey(token.clone())).map_err(|e| e.to_string())?;
    p.authenticate(Credential::ApiKey(token)).await.map_err(|e| e.to_string())?;
    let a = p.account().await.map_err(|e| e.to_string())?;
    state.0.lock().await.provider = Some(p);
    Ok(Acct { username: a.username, premium: a.premium })
}

#[tauri::command]
async fn add(state: tauri::State<'_, AppState>, magnet: String) -> Result<Tx, String> {
    let m = Magnet::parse(&magnet).map_err(|e| e.to_string())?;
    let g = state.0.lock().await;
    let p = g.provider.as_ref().ok_or("connect a provider first")?;
    let t = p.add_magnet(&m).await.map_err(|e| e.to_string())?;
    Ok(to_tx(&t))
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

#[tauri::command]
async fn transfers(state: tauri::State<'_, AppState>) -> Result<Vec<Tx>, String> {
    let g = state.0.lock().await;
    let p = g.provider.as_ref().ok_or("connect a provider first")?;
    let list = p.list_transfers().await.map_err(|e| e.to_string())?;
    Ok(list.iter().map(to_tx).collect())
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

/// iOS share sheet for a downloaded file (includes "Save to Files").
#[tauri::command]
async fn file_share(app: tauri::AppHandle, path: String) -> Result<(), String> {
    app.native().share(&path)
}

/// iOS "Open in…" menu for a downloaded file (VLC, Infuse, …).
#[tauri::command]
async fn file_open_in(app: tauri::AppHandle, path: String) -> Result<(), String> {
    app.native().open_in(&path)
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
        .manage(AppState(Mutex::new(Inner { out_dir, config_dir, ..Default::default() })));

    // Auto-update only in the Ko-fi / direct build; the App Store handles updates itself.
    #[cfg(not(feature = "appstore"))]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build()).setup(|app| {
            use tauri_plugin_updater::UpdaterExt;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(updater) = handle.updater() {
                    if let Ok(Some(update)) = updater.check().await {
                        let _ = update.download_and_install(|_, _| {}, || {}).await;
                    }
                }
            });
            Ok(())
        });
    }

    builder
        .invoke_handler(tauri::generate_handler![
            providers, connect, add, fetch, transfers, download, downloads, links, download_link, open_with,
            reveal, file_share, file_open_in, file_play, native_can_open, license, license_activate,
            license_activate_email
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lidhra");
}
