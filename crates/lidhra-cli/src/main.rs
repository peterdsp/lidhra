//! `lidhra` - the whole pipeline in one command.
//!
//!   magnet  ->  debrid cloud (no P2P on device)  ->  direct HTTPS  ->  segmented download  ->  file
//!
//! Usage:
//!   export DEBRID_TOKEN=your_api_token   # or $RD_TOKEN, or --token
//!   lidhra add "magnet:?xt=urn:btih:...&dn=ubuntu.iso" --provider torbox --out ~/Downloads
//!
//! Options:
//!   --provider <name>    realdebrid (default) | alldebrid | torbox | premiumize
//!   --out <dir>          destination directory (default: current dir)
//!   --token <token>      debrid API token (default: $DEBRID_TOKEN / $RD_TOKEN)
//!   --connections <n>    parallel download connections (default: 4)
//!
//! Any provider works the same way - the CLI builds it via `build_provider`.

use lidhra_debrid::prelude::*;
use lidhra_transfer::{download, DownloadConfig};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("\nlidhra: error: {e}");
        std::process::exit(1);
    }
}

struct Args {
    magnet: String,
    out: PathBuf,
    token: String,
    connections: usize,
    provider: ProviderId,
}

fn usage() -> ! {
    eprintln!(
        "usage: lidhra add <magnet|hash> [--provider <name>] [--out <dir>] [--token <t>] [--connections <n>]\n\
         \n  --provider   realdebrid (default) | alldebrid | torbox | premiumize\n\
         \n  Token comes from --token, else $DEBRID_TOKEN, else $RD_TOKEN."
    );
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut a = std::env::args().skip(1);
    if a.next().as_deref() != Some("add") {
        usage();
    }
    let magnet = a.next().unwrap_or_else(|| usage());
    let mut out = PathBuf::from(".");
    let mut token = std::env::var("DEBRID_TOKEN")
        .or_else(|_| std::env::var("RD_TOKEN"))
        .unwrap_or_default();
    let mut connections = 4usize;
    let mut provider = ProviderId::RealDebrid;
    while let Some(flag) = a.next() {
        match flag.as_str() {
            "--out" => out = PathBuf::from(a.next().unwrap_or_else(|| usage())),
            "--token" => token = a.next().unwrap_or_else(|| usage()),
            "--provider" => {
                provider = a.next().as_deref().and_then(ProviderId::from_key).unwrap_or_else(|| usage())
            }
            "--connections" => {
                connections = a.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| usage())
            }
            _ => usage(),
        }
    }
    if token.is_empty() {
        eprintln!("lidhra: no token - set DEBRID_TOKEN / RD_TOKEN or pass --token");
        std::process::exit(2);
    }
    Args { magnet, out, token, connections, provider }
}

async fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args = parse_args();

    // 1. connect to the chosen provider
    let provider = build_provider(args.provider, Credential::ApiKey(args.token.clone()))?;
    provider.authenticate(Credential::ApiKey(args.token)).await?;
    let acct = provider.account().await?;
    println!("· provider: {} ({}, premium: {})", args.provider.label(), acct.username, acct.premium);

    // 2. hand the magnet to the cloud
    let magnet = Magnet::parse(&args.magnet)?;
    println!("· adding {} …", magnet.display_name.as_deref().unwrap_or(magnet.hash.as_hex()));
    let mut t = provider.add_magnet(&magnet).await?;

    // 3. wait for the cloud to finish torrenting it
    while matches!(t.status, TransferStatus::Queued | TransferStatus::Downloading) {
        print!("\r· cloud: {:?} {:>3.0}%   ", t.status, t.progress * 100.0);
        use std::io::Write;
        std::io::stdout().flush().ok();
        tokio::time::sleep(Duration::from_secs(3)).await;
        t = provider.transfer(&t.id).await?;
    }
    if t.status != TransferStatus::Ready {
        return Err(format!("cloud transfer ended in {:?}", t.status).into());
    }
    println!("\r· cloud: ready ({} file link(s))          ", t.links.len());

    // 4. resolve to direct HTTPS and download each file
    std::fs::create_dir_all(&args.out)?;
    let cfg = DownloadConfig { connections: args.connections, ..Default::default() };
    for link in &t.links {
        let direct = provider.unrestrict(link).await?;
        let dest = args.out.join(sanitize(&direct.filename));
        println!("· downloading {} ({:.1} MB)", direct.filename, direct.size as f64 / 1e6);
        let out = download(&direct.url, &dest, &cfg, progress_printer()).await?;
        println!("\r  ✓ {} - {} bytes via {} connection(s)        ", dest.display(), out.bytes, out.connections);
    }
    println!("· done.");
    Ok(())
}

fn progress_printer() -> impl Fn(lidhra_transfer::Progress) + Send + Sync + 'static {
    |p| {
        use std::io::Write;
        match p.total {
            Some(t) if t > 0 => {
                let pct = p.downloaded as f64 / t as f64 * 100.0;
                let filled = (pct / 5.0) as usize;
                let bar: String = "█".repeat(filled) + &"·".repeat(20 - filled.min(20));
                print!("\r  [{bar}] {pct:>5.1}%");
            }
            _ => print!("\r  {} bytes", p.downloaded),
        }
        std::io::stdout().flush().ok();
    }
}

/// Keep filenames safe for the local filesystem.
fn sanitize(name: &str) -> String {
    let name = Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or(name);
    name.chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { '_' } else { c })
        .collect()
}
