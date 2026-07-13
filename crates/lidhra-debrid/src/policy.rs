use crate::error::{DebridError, Result};
use crate::model::*;
use crate::provider::DebridProvider;

/// How to pick a provider when several could serve a magnet.
#[derive(Clone, Debug)]
pub enum Policy {
    /// Prefer any provider that already has it cached (instant).
    FastestCached,
    /// Try providers in this explicit order.
    PreferredOrder(Vec<ProviderId>),
    /// Just use the first provider that accepts it.
    FirstAvailable,
}

/// Probe the enabled providers for a magnet, rank by `policy`, add on the
/// winner, and fail over to the next on error.
///
/// Returns the provider that accepted it plus the remote transfer handle.
pub async fn resolve<'a>(
    providers: &'a [Box<dyn DebridProvider>],
    magnet: &Magnet,
    policy: &Policy,
) -> Result<(&'a dyn DebridProvider, RemoteTransfer)> {
    if providers.is_empty() {
        return Err(DebridError::Provider("no providers configured".into()));
    }

    // Rank providers per policy.
    let order: Vec<&Box<dyn DebridProvider>> = match policy {
        Policy::FirstAvailable => providers.iter().collect(),
        Policy::PreferredOrder(ids) => {
            let mut ranked: Vec<&Box<dyn DebridProvider>> = Vec::new();
            for id in ids {
                if let Some(p) = providers.iter().find(|p| p.id() == *id) {
                    ranked.push(p);
                }
            }
            // append any not named in the order
            for p in providers {
                if !ranked.iter().any(|r| r.id() == p.id()) {
                    ranked.push(p);
                }
            }
            ranked
        }
        Policy::FastestCached => {
            // Ask each provider whether it has the hash cached; cached first.
            let mut cached = Vec::new();
            let mut uncached = Vec::new();
            for p in providers {
                let hit = p
                    .check_cache(std::slice::from_ref(&magnet.hash))
                    .await
                    .ok()
                    .and_then(|v| v.into_iter().next())
                    .map(|c| c.cached)
                    .unwrap_or(false);
                if hit {
                    cached.push(p);
                } else {
                    uncached.push(p);
                }
            }
            cached.into_iter().chain(uncached).collect()
        }
    };

    // Add on the first that accepts it; fail over on error.
    let mut last_err = DebridError::Provider("all providers failed".into());
    for p in order {
        match p.add_magnet(magnet).await {
            Ok(t) => return Ok((p.as_ref(), t)),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}
