//! End-to-end demo: magnet -> Real-Debrid (cloud torrenting) -> direct HTTPS link.
//!
//! Usage:
//!   export RD_TOKEN=your_real_debrid_api_token
//!   cargo run --example resolve_magnet -- "magnet:?xt=urn:btih:...&dn=ubuntu.iso"
//!
//! Use a LEGITIMATE magnet (a Linux ISO, a Creative-Commons film, your own file).
//! This is exactly the flow an App Review reviewer would see.

use lidhra_debrid::prelude::*;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let token = std::env::var("RD_TOKEN").expect("set RD_TOKEN to a Real-Debrid API token");
    let uri = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "magnet:?xt=urn:btih:2c6b6858d61da9543d4231a71db4b1c9264b0685&dn=example".into());

    let rd = providers::RealDebrid::new(token.clone());
    rd.authenticate(Credential::ApiKey(token)).await?;

    let acct = rd.account().await?;
    println!("account: {} (premium: {})", acct.username, acct.premium);

    let magnet = Magnet::parse(&uri)?;
    println!("adding magnet {} ({})", magnet.hash, magnet.display_name.as_deref().unwrap_or("?"));

    let mut t = rd.add_magnet(&magnet).await?;

    // Poll until the provider finishes torrenting it in the cloud.
    while t.status == TransferStatus::Queued || t.status == TransferStatus::Downloading {
        println!("  {:?} {:.0}%", t.status, t.progress * 100.0);
        tokio::time::sleep(Duration::from_secs(3)).await;
        t = rd.transfer(&t.id).await?;
    }

    if t.status != TransferStatus::Ready {
        eprintln!("transfer ended in {:?}", t.status);
        return Ok(());
    }

    println!("ready - {} direct link(s):", t.links.len());
    for link in &t.links {
        let direct = rd.unrestrict(link).await?;
        // `direct.url` is a plain TLS HTTPS URL - hand it to Lidhra's download engine.
        println!("  {}  ({} bytes)  {}", direct.filename, direct.size, direct.url);
    }
    Ok(())
}
