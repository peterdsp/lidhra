//! Download a URL with a live progress bar.
//!
//!   cargo run --example fetch -- https://releases.ubuntu.com/24.04/SHA256SUMS ./out.txt
//!
//! Point it at a `DirectLink.url` from lidhra-debrid to complete the pipeline
//! (debrid cloud → direct HTTPS → local file).

use lidhra_transfer::{download, DownloadConfig};
use std::path::Path;

#[tokio::main]
async fn main() -> lidhra_transfer::Result<()> {
    let url = std::env::args().nth(1).expect("usage: fetch <url> <dest>");
    let dest = std::env::args().nth(2).unwrap_or_else(|| "download.bin".into());

    let out = download(&url, Path::new(&dest), &DownloadConfig::default(), |p| {
        match p.total {
            Some(t) if t > 0 => {
                let pct = p.downloaded as f64 / t as f64 * 100.0;
                eprint!("\r  {:>6.1}%  {:>10} / {} bytes", pct, p.downloaded, t);
            }
            _ => eprint!("\r  {} bytes", p.downloaded),
        }
    })
    .await?;

    eprintln!("\n  done: {} bytes via {} connection(s) -> {}", out.bytes, out.connections, out.path.display());
    Ok(())
}
