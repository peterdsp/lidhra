//! lidhra-torrent: Lidhra's on-device BitTorrent engine.
//!
//! A small, opinionated layer over `librqbit`: magnets in, files out, plus the
//! policy the app needs and the engine does not provide on its own:
//!
//! - seeding is off by default (a finished torrent is paused, not shared);
//! - torrents pause while the app is suspended or on cellular data;
//! - a free-space check once the size is known;
//! - a session file so torrents survive a restart, re-added relative to the
//!   current download folder (iOS moves the app container between updates).
//!
//! The Tauri app and `lidhra-server` share this crate. Everything here is
//! pure Rust with rustls, so it builds for iOS unchanged.

mod disk;
mod persist;

pub use persist::{SavedTorrent, SETTINGS_FILE, TORRENTS_FILE};

use librqbit::api::TorrentIdOrHash;
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::{
    AddTorrent, AddTorrentOptions, DhtSessionConfig, ListenerOptions, Magnet, ManagedTorrent, Session, SessionOptions,
    TorrentStatsState,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

type ManagedTorrentHandle = Arc<ManagedTorrent>;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a valid magnet link: {0}")]
    BadMagnet(String),
    #[error("no such torrent")]
    NotFound,
    #[error("{0}")]
    Engine(String),
    #[error("the state folder {0} is inside the download folder; a torrent could overwrite the session files")]
    StateInsideDownloads(PathBuf),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

fn engine_err(e: anyhow::Error) -> Error {
    Error::Engine(format!("{e:#}"))
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// User-facing P2P settings. Persisted by the engine in its state folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Keep sharing a finished torrent while the app is open (up to ratio 1.0).
    pub seed: bool,
    /// Pause every torrent while the device is on cellular data.
    pub wifi_only: bool,
    /// Peer connections per torrent (halved in Low Power Mode).
    pub max_connections: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self { seed: false, wifi_only: true, max_connections: 50 }
    }
}

pub const MIN_CONNECTIONS: usize = 2;
pub const MAX_CONNECTIONS: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    /// Fetching metadata for a magnet (no size known yet).
    Resolving,
    /// Hash-checking files already on disk.
    Checking,
    Downloading,
    Seeding,
    Paused,
    /// Finished and not seeding.
    Done,
    Error,
}

/// Why a torrent is paused when the user did not pause it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PauseReason {
    User,
    /// Wi-Fi only is on and the device is on cellular.
    Network,
    /// The app is suspended (iOS background).
    Background,
    /// Not enough free space.
    Disk,
}

#[derive(Clone, Debug, Serialize)]
pub struct Torrent {
    /// Lowercase hex info hash; stable across restarts.
    pub id: String,
    pub name: String,
    pub magnet: String,
    pub state: State,
    pub pause_reason: Option<PauseReason>,
    /// 0.0..=1.0
    pub progress: f32,
    pub downloaded: u64,
    pub total: u64,
    pub uploaded: u64,
    pub down_bps: u64,
    pub up_bps: u64,
    /// Connected peers.
    pub peers: u32,
    /// Peers seen so far from DHT, PEX and trackers.
    pub known_peers: u32,
    pub eta_secs: Option<u64>,
    pub error: Option<String>,
    pub files: usize,
    pub output_folder: PathBuf,
    pub added_at: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TorrentFile {
    pub index: usize,
    /// Path inside the torrent (relative).
    pub name: String,
    /// Absolute location on disk.
    pub path: PathBuf,
    pub size: u64,
    pub downloaded: u64,
    /// Whether this file is selected for download.
    pub included: bool,
    pub done: bool,
}

pub struct EngineConfig {
    /// Where files land (the app's Documents folder on iOS).
    pub out_dir: PathBuf,
    /// Session file, settings and DHT table.
    pub state_dir: PathBuf,
    /// Used only when no settings file exists yet.
    pub settings: Settings,
    pub dht: bool,
    pub trackers: bool,
    /// Incoming-connection listener. `None` binds `[::]:0`.
    pub listen_addr: Option<SocketAddr>,
    /// Sent to peers and trackers as the client name.
    pub client_name: String,
}

impl EngineConfig {
    pub fn new(out_dir: impl Into<PathBuf>, state_dir: impl Into<PathBuf>) -> Self {
        Self {
            out_dir: out_dir.into(),
            state_dir: state_dir.into(),
            settings: Settings::default(),
            dht: true,
            trackers: true,
            listen_addr: None,
            client_name: format!("Lidhra {}", env!("CARGO_PKG_VERSION")),
        }
    }
}

enum AddSpec {
    Magnet(String),
    Bytes(Vec<u8>),
}

struct Entry {
    id: String,
    hash: Id20,
    magnet: String,
    name: Option<String>,
    torrent_bytes: Option<Vec<u8>>,
    only_files: Option<Vec<usize>>,
    /// The user's intention; holds (network, background) are layered on top.
    paused: bool,
    added_at: u64,
    handle: Option<ManagedTorrentHandle>,
    error: Option<String>,
    task: Option<tokio::task::JoinHandle<()>>,
    disk_checked: bool,
}

impl Drop for Entry {
    fn drop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

struct Inner {
    entries: Vec<Entry>,
    settings: Settings,
    /// Torrents paused for a reason other than the user asking.
    held: HashMap<String, PauseReason>,
    cellular: bool,
    suspended: bool,
    low_power: bool,
}

impl Inner {
    fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }
    fn entry_mut(&mut self, id: &str) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }
    /// The hold that currently applies to every torrent, if any.
    fn global_hold(&self) -> Option<PauseReason> {
        if self.suspended {
            Some(PauseReason::Background)
        } else if self.cellular && self.settings.wifi_only {
            Some(PauseReason::Network)
        } else {
            None
        }
    }
    fn peer_limit(&self) -> usize {
        let n = self.settings.max_connections.max(2);
        if self.low_power {
            (n / 2).max(2)
        } else {
            n
        }
    }
    fn saved(&self) -> Vec<SavedTorrent> {
        self.entries
            .iter()
            .map(|e| {
                let mut s = SavedTorrent {
                    id: e.id.clone(),
                    magnet: e.magnet.clone(),
                    name: e.name.clone(),
                    torrent_b64: None,
                    only_files: e.only_files.clone(),
                    paused: e.paused,
                    added_at: e.added_at,
                };
                s.set_torrent_bytes(e.torrent_bytes.as_deref());
                s
            })
            .collect()
    }
}

pub struct Engine {
    session: Arc<Session>,
    inner: Mutex<Inner>,
    out_dir: PathBuf,
    state_dir: PathBuf,
    ticker: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

/// Lowercase hex info hash of a magnet link, or an error for anything else.
pub fn magnet_id(magnet: &str) -> Result<String> {
    parse_magnet(magnet).map(|(h, _)| h.as_string())
}

fn parse_magnet(magnet: &str) -> Result<(Id20, Option<String>)> {
    let m = Magnet::parse(magnet.trim()).map_err(|e| Error::BadMagnet(format!("{e:#}")))?;
    let h = m.as_id20().ok_or_else(|| Error::BadMagnet("no BitTorrent v1 info hash".into()))?;
    Ok((h, m.name))
}

fn file_included(only: Option<&[usize]>, index: usize) -> bool {
    only.map(|o| o.contains(&index)).unwrap_or(true)
}

fn eta_secs(remaining: u64, bps: u64) -> Option<u64> {
    if bps == 0 || remaining == 0 {
        None
    } else {
        Some(remaining.div_ceil(bps))
    }
}

impl Engine {
    /// Start the engine, restoring the previous session. Must run inside a
    /// tokio runtime; librqbit spawns its tasks on it.
    pub async fn start(cfg: EngineConfig) -> Result<Arc<Engine>> {
        std::fs::create_dir_all(&cfg.out_dir)?;
        std::fs::create_dir_all(&cfg.state_dir)?;
        // Peers choose the file names inside the download folder, so the
        // session and settings files must live somewhere they cannot reach.
        let out_canon = std::fs::canonicalize(&cfg.out_dir)?;
        let state_canon = std::fs::canonicalize(&cfg.state_dir)?;
        if state_canon.starts_with(&out_canon) {
            return Err(Error::StateInsideDownloads(cfg.state_dir));
        }
        let mut settings = persist::load_settings(&cfg.state_dir.join(SETTINGS_FILE)).unwrap_or(cfg.settings.clone());
        settings.max_connections = settings.max_connections.clamp(MIN_CONNECTIONS, MAX_CONNECTIONS);

        let mut opts = SessionOptions {
            disable_trackers: !cfg.trackers,
            // Multicast discovery would trigger the iOS Local Network prompt;
            // DHT, PEX and trackers find plenty of peers without it.
            disable_local_service_discovery: true,
            peer_limit: Some(settings.max_connections.max(2)),
            client_name_and_version: Some(cfg.client_name.clone()),
            listen: Some(ListenerOptions {
                listen_addr: cfg.listen_addr.unwrap_or_else(|| (std::net::Ipv6Addr::UNSPECIFIED, 0).into()),
                enable_upnp_port_forwarding: false,
                ..Default::default()
            }),
            ..Default::default()
        };
        opts.dht = if cfg.dht {
            Some(DhtSessionConfig {
                bootstrap_addrs: None,
                port: None,
                persistence: Some(DhtPersistenceConfig {
                    dump_interval: None,
                    config_filename: Some(cfg.state_dir.join("dht.json")),
                }),
            })
        } else {
            None
        };

        let session = Session::new_with_opts(cfg.out_dir.clone(), opts).await.map_err(engine_err)?;
        let engine = Arc::new(Engine {
            session,
            inner: Mutex::new(Inner {
                entries: Vec::new(),
                settings,
                held: HashMap::new(),
                cellular: false,
                suspended: false,
                low_power: false,
            }),
            out_dir: cfg.out_dir,
            state_dir: cfg.state_dir,
            ticker: std::sync::Mutex::new(None),
        });

        for s in persist::load_torrents(&engine.state_dir.join(TORRENTS_FILE)) {
            engine.restore(s).await;
        }

        let weak = Arc::downgrade(&engine);
        let t = tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_secs(2));
            iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                iv.tick().await;
                let Some(e) = weak.upgrade() else { break };
                e.tick().await;
            }
        });
        *engine.ticker.lock().unwrap() = Some(t);
        Ok(engine)
    }

    /// Stop everything and flush the session file. The engine is unusable afterwards.
    pub async fn shutdown(&self) {
        if let Some(t) = self.ticker.lock().unwrap().take() {
            t.abort();
        }
        let g = self.inner.lock().await;
        self.save(&g);
        drop(g);
        self.session.stop().await;
    }

    async fn restore(self: &Arc<Self>, s: SavedTorrent) {
        // Only a real magnet whose hash matches the entry is re-added; librqbit
        // would otherwise treat an http(s) string as a .torrent URL to fetch.
        let Ok((hash, _)) = parse_magnet(&s.magnet) else { return };
        if hash.as_string() != s.id {
            return;
        }
        let spec = match s.torrent_bytes() {
            Some(b) => AddSpec::Bytes(b),
            None => AddSpec::Magnet(s.magnet.clone()),
        };
        let mut g = self.inner.lock().await;
        if g.entry(&s.id).is_some() {
            return;
        }
        g.entries.push(Entry {
            id: s.id.clone(),
            hash,
            magnet: s.magnet,
            name: s.name,
            torrent_bytes: None, // re-captured from the handle so the file stays fresh
            only_files: s.only_files.clone(),
            paused: s.paused,
            added_at: s.added_at,
            handle: None,
            error: None,
            task: None,
            disk_checked: false,
        });
        let task = self.spawn_add(s.id.clone(), spec, s.paused, s.only_files, Vec::new(), g.peer_limit());
        if let Some(e) = g.entry_mut(&s.id) {
            e.task = Some(task);
        }
    }

    /// Add a magnet. Returns at once with the torrent in `Resolving`; metadata,
    /// peers and progress arrive through `list`/`get`.
    pub async fn add_magnet(self: &Arc<Self>, magnet: &str) -> Result<Torrent> {
        self.add_magnet_with_peers(magnet, Vec::new()).await
    }

    /// Like `add_magnet`, with peers to try first (tests, or a known seeder).
    pub async fn add_magnet_with_peers(self: &Arc<Self>, magnet: &str, peers: Vec<SocketAddr>) -> Result<Torrent> {
        let magnet = magnet.trim().to_string();
        let (hash, name) = parse_magnet(&magnet)?;
        let id = hash.as_string();
        let mut g = self.inner.lock().await;
        if let Some(e) = g.entry(&id) {
            return Ok(self.snapshot(&g, e));
        }
        g.entries.push(Entry {
            id: id.clone(),
            hash,
            magnet: magnet.clone(),
            name,
            torrent_bytes: None,
            only_files: None,
            paused: false,
            added_at: now_unix(),
            handle: None,
            error: None,
            task: None,
            disk_checked: false,
        });
        let task = self.spawn_add(id.clone(), AddSpec::Magnet(magnet), false, None, peers, g.peer_limit());
        let e = g.entry_mut(&id).expect("just pushed");
        e.task = Some(task);
        self.save(&g);
        let e = g.entry(&id).expect("just pushed");
        Ok(self.snapshot(&g, e))
    }

    fn spawn_add(
        self: &Arc<Self>,
        id: String,
        spec: AddSpec,
        paused: bool,
        only_files: Option<Vec<usize>>,
        peers: Vec<SocketAddr>,
        peer_limit: usize,
    ) -> tokio::task::JoinHandle<()> {
        Self::spawn_add_inner(Arc::downgrade(self), self.session.clone(), id, spec, paused, only_files, peers, peer_limit)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_add_inner(
        weak: Weak<Engine>,
        session: Arc<Session>,
        id: String,
        spec: AddSpec,
        paused: bool,
        only_files: Option<Vec<usize>>,
        peers: Vec<SocketAddr>,
        peer_limit: usize,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let opts = AddTorrentOptions {
                paused,
                overwrite: true,
                only_files,
                initial_peers: if peers.is_empty() { None } else { Some(peers) },
                peer_limit: Some(peer_limit),
                ..Default::default()
            };
            let add = match spec {
                AddSpec::Magnet(m) => AddTorrent::from_url(m),
                AddSpec::Bytes(b) => AddTorrent::from_bytes(b),
            };
            let res = session.add_torrent(add, Some(opts)).await;
            let Some(engine) = weak.upgrade() else { return };
            let mut g = engine.inner.lock().await;
            match g.entry_mut(&id) {
                None => {
                    // Removed while it was still resolving: drop it from the session too.
                    if let Ok(r) = res {
                        if let Some(h) = r.into_handle() {
                            let _ = session.delete(TorrentIdOrHash::Hash(h.info_hash()), false).await;
                        }
                    }
                }
                Some(e) => {
                    match res {
                        Ok(r) => {
                            e.handle = r.into_handle();
                            e.error = None;
                        }
                        Err(err) => e.error = Some(format!("{err:#}")),
                    }
                    e.task = None;
                    engine.apply_holds(&mut g).await;
                }
            }
        })
    }

    pub async fn list(&self) -> Vec<Torrent> {
        let g = self.inner.lock().await;
        g.entries.iter().map(|e| self.snapshot(&g, e)).collect()
    }

    pub async fn get(&self, id: &str) -> Option<Torrent> {
        let g = self.inner.lock().await;
        g.entry(id).map(|e| self.snapshot(&g, e))
    }

    pub async fn files(&self, id: &str) -> Result<Vec<TorrentFile>> {
        let g = self.inner.lock().await;
        let e = g.entry(id).ok_or(Error::NotFound)?;
        let Some(h) = &e.handle else { return Ok(Vec::new()) };
        let only = h.only_files();
        let st = h.stats();
        let out = h.output_folder().to_path_buf();
        h.with_metadata(|m| {
            m.file_infos
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let downloaded = st.file_progress.get(i).copied().unwrap_or(0);
                    TorrentFile {
                        index: i,
                        name: f.relative_filename.to_string_lossy().into_owned(),
                        path: out.join(&f.relative_filename),
                        size: f.len,
                        downloaded,
                        included: file_included(only.as_deref(), i),
                        done: downloaded >= f.len,
                    }
                })
                .collect()
        })
        .or_else(|_| Ok(Vec::new()))
    }

    pub async fn pause(&self, id: &str) -> Result<()> {
        let mut g = self.inner.lock().await;
        let e = g.entry_mut(id).ok_or(Error::NotFound)?;
        e.paused = true;
        if let Some(h) = e.handle.clone().filter(|h| !h.is_paused()) {
            self.session.pause(&h).await.map_err(engine_err)?;
        }
        g.held.remove(id);
        self.save(&g);
        Ok(())
    }

    pub async fn resume(&self, id: &str) -> Result<()> {
        let mut g = self.inner.lock().await;
        let e = g.entry_mut(id).ok_or(Error::NotFound)?;
        e.paused = false;
        e.error = None;
        e.disk_checked = false;
        g.held.remove(id);
        self.save(&g);
        self.apply_holds(&mut g).await;
        Ok(())
    }

    /// Remove a torrent, optionally deleting its files from disk.
    pub async fn remove(&self, id: &str, delete_files: bool) -> Result<()> {
        let mut g = self.inner.lock().await;
        let pos = g.entries.iter().position(|e| e.id == id).ok_or(Error::NotFound)?;
        let e = g.entries.remove(pos);
        g.held.remove(id);
        self.save(&g);
        drop(g);
        if let Some(t) = &e.task {
            t.abort();
        }
        // Not in the session yet (still resolving) is not an error.
        let _ = self.session.delete(TorrentIdOrHash::Hash(e.hash), delete_files).await;
        Ok(())
    }

    pub async fn settings(&self) -> Settings {
        self.inner.lock().await.settings.clone()
    }

    pub async fn set_settings(&self, mut s: Settings) -> Result<()> {
        s.max_connections = s.max_connections.clamp(MIN_CONNECTIONS, MAX_CONNECTIONS);
        let mut g = self.inner.lock().await;
        let seed_was = g.settings.seed;
        g.settings = s;
        persist::save_settings(&self.state_dir.join(SETTINGS_FILE), &g.settings)?;
        if seed_was && !g.settings.seed {
            // Stop sharing finished torrents right away.
            let done: Vec<_> = g
                .entries
                .iter()
                .filter_map(|e| e.handle.clone().filter(|h| h.stats().finished && !h.is_paused()))
                .collect();
            for h in done {
                let _ = self.session.pause(&h).await;
            }
        }
        self.apply_holds(&mut g).await;
        Ok(())
    }

    /// iOS: the app is about to be suspended (or came back). Pauses every
    /// torrent cleanly and saves, so nothing is mid-write when the process freezes.
    pub async fn set_suspended(&self, suspended: bool) {
        let mut g = self.inner.lock().await;
        g.suspended = suspended;
        self.apply_holds(&mut g).await;
        if suspended {
            self.save(&g);
        }
    }

    /// Current network: `true` while on cellular (or another expensive path).
    pub async fn set_cellular(&self, cellular: bool) {
        let mut g = self.inner.lock().await;
        g.cellular = cellular;
        self.apply_holds(&mut g).await;
    }

    /// Low Power Mode halves the connection count for torrents added from now on.
    pub async fn set_low_power(&self, on: bool) {
        self.inner.lock().await.low_power = on;
    }

    /// Bring every torrent in line with the user's intention plus the current
    /// hold (background, cellular). Finished torrents stay paused unless seeding.
    async fn apply_holds(&self, g: &mut Inner) {
        let hold = g.global_hold();
        let seed = g.settings.seed;
        let mut pause = Vec::new();
        let mut unpause = Vec::new();
        for e in &g.entries {
            let Some(h) = e.handle.clone() else { continue };
            let held = g.held.get(&e.id).copied();
            if e.paused || held == Some(PauseReason::Disk) {
                continue;
            }
            let st = h.stats();
            let finished = st.finished;
            match hold {
                Some(r) => {
                    if !h.is_paused() {
                        pause.push((e.id.clone(), h, r));
                    } else if held.is_none() && !finished {
                        // Paused by librqbit (e.g. initializing) but not by us; record the hold anyway.
                        pause.push((e.id.clone(), h, r));
                    }
                }
                None => {
                    if h.is_paused() && (!finished || seed) && !matches!(st.state, TorrentStatsState::Error) {
                        unpause.push((e.id.clone(), h));
                    }
                }
            }
        }
        for (id, h, r) in pause {
            let _ = self.session.pause(&h).await;
            g.held.insert(id, r);
        }
        for (id, h) in unpause {
            if self.session.unpause(&h).await.is_ok() {
                g.held.remove(&id);
            }
        }
    }

    /// Periodic housekeeping: capture metadata once known, check free space,
    /// enforce the seeding policy, and persist changes.
    async fn tick(&self) {
        let mut g = self.inner.lock().await;
        let seed = g.settings.seed;
        let mut dirty = false;
        let mut to_pause: Vec<(String, ManagedTorrentHandle, Option<PauseReason>)> = Vec::new();
        for e in g.entries.iter_mut() {
            let Some(h) = e.handle.clone() else { continue };
            if e.torrent_bytes.is_none() {
                if let Ok((bytes, name)) = h.with_metadata(|m| (m.torrent_bytes.to_vec(), m.info.name().map(|n| n.into_owned()))) {
                    e.torrent_bytes = Some(bytes);
                    if name.is_some() {
                        e.name = name;
                    }
                    dirty = true;
                }
            }
            let st = h.stats();
            if !e.disk_checked && st.total_bytes > 0 && !st.finished {
                e.disk_checked = true;
                let need = st.total_bytes.saturating_sub(st.progress_bytes);
                if let Some(free) = disk::free_space(&self.out_dir) {
                    if need > free {
                        e.error = Some(format!("needs {} free, {} available", disk::human(need), disk::human(free)));
                        to_pause.push((e.id.clone(), h.clone(), Some(PauseReason::Disk)));
                        continue;
                    }
                }
            }
            if st.finished && !e.paused && !h.is_paused() && matches!(st.state, TorrentStatsState::Live) {
                let ratio_done = st.uploaded_bytes >= st.total_bytes.max(1);
                if !seed || ratio_done {
                    to_pause.push((e.id.clone(), h.clone(), None));
                }
            }
        }
        for (id, h, reason) in to_pause {
            let _ = self.session.pause(&h).await;
            if let Some(r) = reason {
                g.held.insert(id, r);
            }
        }
        if dirty {
            self.save(&g);
        }
    }

    fn save(&self, g: &Inner) {
        let _ = persist::save_torrents(&self.state_dir.join(TORRENTS_FILE), &g.saved());
    }

    fn snapshot(&self, g: &Inner, e: &Entry) -> Torrent {
        let held = g.held.get(&e.id).copied();
        let mut t = Torrent {
            id: e.id.clone(),
            name: e.name.clone().unwrap_or_else(|| e.id.clone()),
            magnet: e.magnet.clone(),
            state: State::Resolving,
            pause_reason: None,
            progress: 0.0,
            downloaded: 0,
            total: 0,
            uploaded: 0,
            down_bps: 0,
            up_bps: 0,
            peers: 0,
            known_peers: 0,
            eta_secs: None,
            error: e.error.clone(),
            files: 0,
            output_folder: self.out_dir.clone(),
            added_at: e.added_at,
        };
        let Some(h) = &e.handle else {
            t.state = if e.error.is_some() {
                State::Error
            } else if e.paused {
                t.pause_reason = Some(PauseReason::User);
                State::Paused
            } else {
                State::Resolving
            };
            return t;
        };
        if let Some(n) = h.name() {
            t.name = n;
        }
        t.output_folder = h.output_folder().to_path_buf();
        t.files = h.with_metadata(|m| m.file_infos.len()).unwrap_or(0);
        let st = h.stats();
        t.downloaded = st.progress_bytes;
        t.total = st.total_bytes;
        t.uploaded = st.uploaded_bytes;
        t.progress = if st.total_bytes > 0 { (st.progress_bytes as f32 / st.total_bytes as f32).clamp(0.0, 1.0) } else { 0.0 };
        if let Some(l) = &st.live {
            t.down_bps = l.download_speed.as_bytes();
            t.up_bps = l.upload_speed.as_bytes();
            t.peers = l.snapshot.peer_stats.live;
            t.known_peers = l.snapshot.peer_stats.seen;
        }
        t.eta_secs = eta_secs(st.total_bytes.saturating_sub(st.progress_bytes), t.down_bps);
        if st.error.is_some() {
            t.error = st.error.clone();
        }
        t.state = match st.state {
            TorrentStatsState::Initializing { .. } => State::Checking,
            TorrentStatsState::Error => State::Error,
            TorrentStatsState::Paused => {
                if st.finished {
                    State::Done
                } else {
                    t.pause_reason = held.or(if e.paused { Some(PauseReason::User) } else { None });
                    State::Paused
                }
            }
            TorrentStatsState::Live => {
                if st.finished {
                    State::Seeding
                } else {
                    State::Downloading
                }
            }
        };
        t
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(t) = self.ticker.lock().unwrap().take() {
            t.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";

    #[test]
    fn magnet_id_is_lowercase_hex_of_the_info_hash() {
        let m = format!("magnet:?xt=urn:btih:{}&dn=ubuntu.iso&tr=udp://tracker.example:6969", HASH.to_uppercase());
        assert_eq!(magnet_id(&m).unwrap(), HASH);
        assert_eq!(magnet_id(&format!("  magnet:?xt=urn:btih:{HASH}  ")).unwrap(), HASH, "whitespace is trimmed");
    }

    #[test]
    fn magnet_name_comes_from_dn() {
        let (h, name) = parse_magnet(&format!("magnet:?xt=urn:btih:{HASH}&dn=Big%20Buck%20Bunny")).unwrap();
        assert_eq!(h.as_string(), HASH);
        assert_eq!(name.as_deref(), Some("Big Buck Bunny"));
    }

    #[test]
    fn rejects_non_magnets() {
        for bad in ["https://example.com/file.iso", "", "magnet:?dn=no-hash", "magnet:?xt=urn:btih:nothex"] {
            assert!(matches!(magnet_id(bad), Err(Error::BadMagnet(_))), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn file_selection() {
        assert!(file_included(None, 7), "no selection means every file");
        assert!(file_included(Some(&[0, 2]), 2));
        assert!(!file_included(Some(&[0, 2]), 1));
        assert!(!file_included(Some(&[]), 0));
    }

    #[test]
    fn eta_rounds_up_and_handles_idle() {
        assert_eq!(eta_secs(1000, 0), None);
        assert_eq!(eta_secs(0, 100), None);
        assert_eq!(eta_secs(1000, 300), Some(4));
        assert_eq!(eta_secs(900, 300), Some(3));
    }

    #[test]
    fn holds_and_peer_limits() {
        let mut i = Inner {
            entries: Vec::new(),
            settings: Settings::default(),
            held: HashMap::new(),
            cellular: false,
            suspended: false,
            low_power: false,
        };
        assert_eq!(i.global_hold(), None);
        i.cellular = true;
        assert_eq!(i.global_hold(), Some(PauseReason::Network), "wifi_only is on by default");
        i.settings.wifi_only = false;
        assert_eq!(i.global_hold(), None);
        i.suspended = true;
        assert_eq!(i.global_hold(), Some(PauseReason::Background), "background wins over everything");
        assert_eq!(i.peer_limit(), 50);
        i.low_power = true;
        assert_eq!(i.peer_limit(), 25);
        i.settings.max_connections = 1;
        assert_eq!(i.peer_limit(), 2, "never below two peers");
    }
}
