//! lidhra-debrid — unified interface over every debrid / multi-hoster provider.
//!
//! Design goal: adding a provider (Real-Debrid, AllDebrid, TorBox, Premiumize,
//! Debrid-Link, Offcloud, Mega-Debrid, Deepbrid, High-Way, …) means writing ONE
//! adapter that implements `DebridProvider`. Nothing else in Lidhra changes.
//!
//! This file is an illustrative starter skeleton, not a compiled crate.
//! See Strategy/DEBRID-AND-APPSTORE-STRATEGY.md for the full plan.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------- identity & capabilities ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderId {
    RealDebrid, AllDebrid, TorBox, Premiumize,
    DebridLink, Offcloud, MegaDebrid, Deepbrid, HighWay,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities {
    pub magnet: bool,
    pub torrent_file: bool,
    pub batch_cache_check: bool, // can check many hashes in one call (RD, TorBox, Premiumize)
    pub streaming_link: bool,
    pub folders: bool,
}

// ---------- credentials & account ----------

/// Bring-Your-Own-Account only. Lidhra never ships or brokers accounts.
#[derive(Clone, Debug)]
pub enum Credential {
    ApiKey(String),
    OAuth { access: String, refresh: String }, // e.g. Real-Debrid device-code flow
}

#[derive(Clone, Debug)]
pub struct AccountInfo {
    pub username: String,
    pub premium: bool,
    pub expires_at: Option<i64>, // unix seconds
    pub traffic_left: Option<u64>,
}

// ---------- transfer / link models ----------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoHash(pub [u8; 20]);

#[derive(Clone, Debug)]
pub struct Magnet {
    pub hash: InfoHash,
    pub uri: String,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CacheStatus {
    pub hash: InfoHash,
    pub cached: bool, // "instant" if already on the provider
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferStatus { Queued, Downloading, Ready, Error }

#[derive(Clone, Debug)]
pub struct TransferId(pub String);

#[derive(Clone, Debug)]
pub struct RemoteTransfer {
    pub id: TransferId,
    pub name: String,
    pub status: TransferStatus,
    pub progress: f32,             // 0.0..=1.0
    pub links: Vec<RestrictedLink>, // resolve to DirectLink via `unrestrict`
}

#[derive(Clone, Debug)]
pub struct RestrictedLink(pub String);

/// The end goal: a plain TLS HTTPS URL handed to lidhra-transfer.
#[derive(Clone, Debug)]
pub struct DirectLink {
    pub url: String,        // https://…  (always TLS)
    pub filename: String,
    pub size: u64,
    pub mime: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum DebridError {
    #[error("auth failed")] Auth,
    #[error("rate limited")] RateLimited,
    #[error("not cached")] NotCached,
    #[error("provider error: {0}")] Provider(String),
    #[error(transparent)] Transport(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, DebridError>;

// ---------- the one trait every provider implements ----------

#[async_trait]
pub trait DebridProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> Capabilities;

    async fn authenticate(&self, cred: Credential) -> Result<()>;
    async fn account(&self) -> Result<AccountInfo>;

    /// Which of these hashes are already cached (instant)? One call if the
    /// provider supports batch checking, else fan out.
    async fn check_cache(&self, hashes: &[InfoHash]) -> Result<Vec<CacheStatus>>;

    async fn add_magnet(&self, magnet: &Magnet) -> Result<RemoteTransfer>;
    async fn add_torrent(&self, torrent: &[u8]) -> Result<RemoteTransfer>;

    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>>;
    async fn transfer(&self, id: &TransferId) -> Result<RemoteTransfer>;

    /// Restricted → direct HTTPS. This is the hand-off to the download engine.
    async fn unrestrict(&self, link: &RestrictedLink) -> Result<DirectLink>;

    async fn delete(&self, id: &TransferId) -> Result<()>;
}

// ---------- example adapter (stub) ----------

pub struct RealDebrid { http: reqwest::Client, token: parking_lot::RwLock<Option<String>> }

#[async_trait]
impl DebridProvider for RealDebrid {
    fn id(&self) -> ProviderId { ProviderId::RealDebrid }
    fn capabilities(&self) -> Capabilities {
        Capabilities { magnet: true, torrent_file: true, batch_cache_check: true,
                        streaming_link: true, folders: true }
    }
    async fn authenticate(&self, cred: Credential) -> Result<()> {
        // POST /oauth or store api key; validate via /user
        let _ = cred; Ok(())
    }
    async fn account(&self) -> Result<AccountInfo> { todo!("GET /user") }
    async fn check_cache(&self, _hashes: &[InfoHash]) -> Result<Vec<CacheStatus>> {
        todo!("GET /torrents/instantAvailability/{hash..}")
    }
    async fn add_magnet(&self, _m: &Magnet) -> Result<RemoteTransfer> {
        todo!("POST /torrents/addMagnet → selectFiles → poll")
    }
    async fn add_torrent(&self, _d: &[u8]) -> Result<RemoteTransfer> { todo!("PUT /torrents/addTorrent") }
    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>> { todo!("GET /torrents") }
    async fn transfer(&self, _id: &TransferId) -> Result<RemoteTransfer> { todo!("GET /torrents/info/{id}") }
    async fn unrestrict(&self, _l: &RestrictedLink) -> Result<DirectLink> { todo!("POST /unrestrict/link") }
    async fn delete(&self, _id: &TransferId) -> Result<()> { todo!("DELETE /torrents/delete/{id}") }
}

// ---------- policy engine: choose a provider per hash ----------

pub enum Policy { FastestCached, CheapestTraffic, PreferredOrder(Vec<ProviderId>), RoundRobin }

/// Given the enabled providers and a magnet, pick where to resolve it:
/// prefer a provider that already has it cached (instant), else fall back
/// to policy order; on failure, fail over to the next provider.
pub async fn resolve<'a>(
    providers: &'a [Box<dyn DebridProvider>],
    magnet: &Magnet,
    policy: &Policy,
) -> Result<(&'a dyn DebridProvider, RemoteTransfer)> {
    // 1. probe cache across all providers in parallel
    // 2. rank by `policy`
    // 3. add_magnet on the winner; on error, try the next
    let _ = (providers, magnet, policy);
    todo!("cache probe → rank → add → failover")
}
