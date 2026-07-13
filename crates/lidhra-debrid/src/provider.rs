use crate::error::Result;
use crate::model::*;
use async_trait::async_trait;

/// The one trait every debrid provider implements.
///
/// The rest of Lidhra depends only on this — swap or add providers freely.
#[async_trait]
pub trait DebridProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> Capabilities;

    /// Validate/store credentials. Returns Err([`crate::DebridError::Auth`]) on bad tokens.
    async fn authenticate(&self, cred: Credential) -> Result<()>;

    /// Account status (premium, expiry, traffic).
    async fn account(&self) -> Result<AccountInfo>;

    /// Which of these info-hashes are already cached (instant)?
    ///
    /// Providers without a cache endpoint (or that deprecated it) may return
    /// `cached: false` for all — callers then fall back to add + poll.
    async fn check_cache(&self, hashes: &[InfoHash]) -> Result<Vec<CacheStatus>>;

    /// Submit a magnet; the provider torrents it in the cloud.
    async fn add_magnet(&self, magnet: &Magnet) -> Result<RemoteTransfer>;

    /// Submit a raw `.torrent` file.
    async fn add_torrent(&self, torrent: &[u8]) -> Result<RemoteTransfer>;

    async fn list_transfers(&self) -> Result<Vec<RemoteTransfer>>;
    async fn transfer(&self, id: &TransferId) -> Result<RemoteTransfer>;

    /// Restricted link → direct HTTPS URL. The hand-off to the download engine.
    async fn unrestrict(&self, link: &RestrictedLink) -> Result<DirectLink>;

    async fn delete(&self, id: &TransferId) -> Result<()>;
}
