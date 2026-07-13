//! Lidhra desktop (Tauri) - native shell over the shared web UI (`ui/`).
//!
//! The same `ui/index.html` the server serves runs here too; when it detects a
//! Tauri window it calls these `#[tauri::command]`s via `invoke` instead of HTTP.
//! Commands drive the `lidhra-debrid` + `lidhra-transfer` crates directly.

use lidhra_debrid::prelude::*;
use lidhra_transfer::{download as fetch_file, DownloadConfig};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::result::Result; // shadow the prelude's `Result` alias back to std's
use tokio::sync::Mutex;

#[derive(Default)]
struct Inner {
    provider: Option<Box<dyn DebridProvider>>,
    out_dir: Option<PathBuf>,
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
fn sanitize(name: &str) -> String {
    let n = Path::new(name).file_name().and_then(|x| x.to_str()).unwrap_or(name);
    n.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { '_' } else { c }).collect()
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
async fn transfers(state: tauri::State<'_, AppState>) -> Result<Vec<Tx>, String> {
    let g = state.0.lock().await;
    let p = g.provider.as_ref().ok_or("connect a provider first")?;
    let list = p.list_transfers().await.map_err(|e| e.to_string())?;
    Ok(list.iter().map(to_tx).collect())
}

#[tauri::command]
async fn download(state: tauri::State<'_, AppState>, id: String) -> Result<usize, String> {
    let (links, out) = {
        let g = state.0.lock().await;
        let p = g.provider.as_ref().ok_or("connect a provider first")?;
        let t = p.transfer(&TransferId(id)).await.map_err(|e| e.to_string())?;
        let mut direct = Vec::new();
        for l in &t.links {
            if let Ok(d) = p.unrestrict(l).await {
                direct.push(d);
            }
        }
        (direct, g.out_dir.clone().unwrap_or_else(|| PathBuf::from(".")))
    };
    let n = links.len();
    std::fs::create_dir_all(&out).ok();
    for d in links {
        let out = out.clone();
        tauri::async_runtime::spawn(async move {
            let dest = out.join(sanitize(&d.filename));
            let _ = fetch_file(&d.url, &dest, &DownloadConfig::default(), |_p| {}).await;
        });
    }
    Ok(n)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let out_dir = std::env::var("HOME").ok().map(|h| PathBuf::from(h).join("Downloads"));
    tauri::Builder::default()
        .manage(AppState(Mutex::new(Inner { provider: None, out_dir })))
        .invoke_handler(tauri::generate_handler![providers, connect, add, transfers, download])
        .run(tauri::generate_context!())
        .expect("error while running Lidhra");
}
