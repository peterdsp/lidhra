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
use tokio::sync::Mutex;

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// One local file download, shared with its background task via atomics.
struct Dl {
    id: String,
    name: String,
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
    let dl = Arc::new(Dl {
        id: format!("d{}", inner.next_id),
        name,
        downloaded: AtomicU64::new(0),
        total: AtomicU64::new(0),
        done: AtomicBool::new(false),
        error: std::sync::Mutex::new(None),
    });
    inner.next_id += 1;
    inner.downloads.push(dl.clone());
    let out = inner.out_dir.clone().unwrap_or_else(|| PathBuf::from("."));
    let handle = dl.clone();
    tauri::async_runtime::spawn(async move {
        let dest = out.join(&handle.name);
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

#[derive(Serialize)]
struct Lic {
    state: String,
    days_left: u32,
    owner: Option<String>,
}

#[cfg(not(feature = "appstore"))]
fn lic_dto(dir: &Path) -> Lic {
    let install = lidhra_license::load_or_init_install(dir, now_unix());
    let lic = lidhra_license::load_license(dir);
    match lidhra_license::status(now_unix(), install, lic.as_deref(), lidhra_license::ISSUER_PUBKEY_HEX) {
        lidhra_license::Status::Licensed { owner } => Lic { state: "licensed".into(), days_left: 0, owner: Some(owner) },
        lidhra_license::Status::Trial { days_left } => Lic { state: "trial".into(), days_left, owner: None },
        lidhra_license::Status::Expired => Lic { state: "expired".into(), days_left: 0, owner: None },
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

// App Store build: paid upfront, always licensed, no trial.
#[cfg(feature = "appstore")]
#[tauri::command]
async fn license(_s: tauri::State<'_, AppState>) -> Result<Lic, String> {
    Ok(Lic { state: "licensed".into(), days_left: 0, owner: Some("App Store".into()) })
}
#[cfg(feature = "appstore")]
#[tauri::command]
async fn license_activate(_s: tauri::State<'_, AppState>, _key: String) -> Result<Lic, String> {
    Ok(Lic { state: "licensed".into(), days_left: 0, owner: Some("App Store".into()) })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let out_dir = std::env::var("HOME").ok().map(|h| PathBuf::from(h).join("Downloads"));
    let config_dir = std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config").join("lidhra"))
        .unwrap_or_else(|| PathBuf::from(".lidhra"));

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
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
            providers, connect, add, fetch, transfers, download, downloads, license, license_activate
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lidhra");
}
