//! # lidhra-debrid
//!
//! A unified, async interface over debrid / multi-hoster services
//! (Real-Debrid, AllDebrid, TorBox, Premiumize, Debrid-Link, Offcloud, …).
//!
//! The whole point: the rest of Lidhra talks to [`DebridProvider`] and never
//! knows which service is behind it. Adding a provider = writing one adapter.
//!
//! The flow, in one picture:
//! ```text
//! magnet / hash / .torrent
//!        │  check_cache()  (which providers already have it?)
//!        ▼  add_magnet()   (provider torrents it in the cloud — no P2P on device)
//!        ▼  unrestrict()   (restricted link → direct HTTPS URL, TLS)
//!        ▼  hand the DirectLink to Lidhra's HTTPS download engine
//! ```
//!
//! ## Example
//! ```no_run
//! use lidhra_debrid::prelude::*;
//! # async fn run() -> Result<()> {
//! let rd = providers::RealDebrid::new(std::env::var("RD_TOKEN").unwrap());
//! rd.authenticate(Credential::ApiKey(std::env::var("RD_TOKEN").unwrap())).await?;
//! let magnet = Magnet::parse("magnet:?xt=urn:btih:...&dn=ubuntu.iso")?;
//! let t = rd.add_magnet(&magnet).await?;
//! // poll rd.transfer(&t.id) until Ready, then:
//! for link in t.links { println!("{}", rd.unrestrict(&link).await?.url); }
//! # Ok(()) }
//! ```

mod error;
mod model;
mod provider;
mod policy;
mod registry;
pub mod providers;

pub use error::{DebridError, Result};
pub use model::{
    Account, AccountInfo, Capabilities, CacheStatus, Credential, DirectLink, InfoHash, Magnet,
    ProviderId, RemoteTransfer, RestrictedLink, TransferId, TransferStatus,
};
pub use policy::{resolve, Policy};
pub use provider::DebridProvider;
pub use registry::build_provider;

/// Glob-import everything you usually need.
pub mod prelude {
    pub use crate::{
        build_provider, providers, resolve, AccountInfo, Capabilities, CacheStatus, Credential,
        DebridError, DebridProvider, DirectLink, InfoHash, Magnet, Policy, ProviderId,
        RemoteTransfer, Result, TransferId, TransferStatus,
    };
}
