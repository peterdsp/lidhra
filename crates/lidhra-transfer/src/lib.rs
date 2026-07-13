//! # lidhra-transfer
//!
//! A segmented, **resumable** HTTPS download engine. Feed it a direct link (e.g.
//! the `DirectLink.url` from `lidhra-debrid`) and it downloads to a file over TLS,
//! splitting across parallel HTTP Range connections when the server allows it —
//! the last step of the Lidhra pipeline.
//!
//! Downloads land in a `<dest>.part` file and are atomically renamed to `<dest>`
//! only on success, so an interrupted run never leaves a half-written file in
//! place. If a `.part` survives and the server supports ranges, the next run
//! resumes from where it left off.
//!
//! ```no_run
//! use lidhra_transfer::{download, DownloadConfig};
//! # async fn run() -> lidhra_transfer::Result<()> {
//! let out = download(
//!     "https://example.com/ubuntu.iso",
//!     std::path::Path::new("ubuntu.iso"),
//!     &DownloadConfig::default(),
//!     |p| eprintln!("{}/{:?}", p.downloaded, p.total),
//! ).await?;
//! println!("saved {} bytes to {}", out.bytes, out.path.display());
//! # Ok(()) }
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use thiserror::Error;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

#[derive(Error, Debug)]
pub enum TransferError {
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("a download segment task panicked")]
    Join,
}

pub type Result<T> = std::result::Result<T, TransferError>;

#[derive(Clone, Debug)]
pub struct DownloadConfig {
    /// Number of parallel Range connections when the server supports them.
    pub connections: usize,
    /// Don't bother splitting files smaller than this (bytes).
    pub min_split: u64,
    /// Resume from an existing `<dest>.part` when possible.
    pub resume: bool,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        DownloadConfig { connections: 4, min_split: 4 * 1024 * 1024, resume: true }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Progress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub path: PathBuf,
    pub bytes: u64,
    /// Connections used: N for segmented, 1 for single-stream, 0 if already complete.
    pub connections: usize,
    /// Whether this run resumed a partial `.part` file.
    pub resumed: bool,
}

/// `<dest>` -> `<dest>.part`
pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Split `total` bytes into `n` contiguous inclusive `(start, end)` ranges.
pub fn split_ranges(total: u64, n: usize) -> Vec<(u64, u64)> {
    let n = n.max(1) as u64;
    if total == 0 {
        return vec![(0, 0)];
    }
    // Can't have more segments than bytes.
    let n = n.min(total);
    let chunk = total / n;
    let mut ranges = Vec::with_capacity(n as usize);
    let mut start = 0u64;
    for i in 0..n {
        let end = if i == n - 1 { total - 1 } else { start + chunk - 1 };
        ranges.push((start, end));
        start = end + 1;
    }
    ranges
}

/// Download `url` to `dest`: parallel Range segments when possible, single-stream
/// otherwise, resuming a `<dest>.part` if one is present. Reports progress via
/// `on_progress`, and atomically renames `.part` -> `dest` on success.
pub async fn download<F>(
    url: &str,
    dest: &Path,
    cfg: &DownloadConfig,
    on_progress: F,
) -> Result<Outcome>
where
    F: Fn(Progress) + Send + Sync + 'static,
{
    let client = reqwest::Client::builder().build()?;
    let on_progress: Arc<dyn Fn(Progress) + Send + Sync> = Arc::new(on_progress);
    let part = part_path(dest);

    let (total, ranges_ok) = probe(&client, url).await;
    let existing = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);

    // Already fully present in .part — just finalize.
    if let Some(t) = total {
        if existing == t && t > 0 {
            tokio::fs::rename(&part, dest).await?;
            on_progress(Progress { downloaded: t, total });
            return Ok(Outcome { path: dest.to_path_buf(), bytes: t, connections: 0, resumed: true });
        }
    }

    let can_resume =
        cfg.resume && existing > 0 && ranges_ok && total.map(|t| existing < t).unwrap_or(true);

    let (bytes, connections, resumed) = if can_resume {
        let b = single_stream(&client, url, &part, total, existing, on_progress).await?;
        (b, 1, true)
    } else {
        // Fresh download — discard any stale partial.
        let _ = tokio::fs::remove_file(&part).await;
        let segmented = ranges_ok
            && cfg.connections > 1
            && total.map(|t| t >= cfg.min_split).unwrap_or(false);
        if segmented {
            let t = total.unwrap();
            segmented_download(&client, url, &part, cfg.connections, t, on_progress).await?;
            (t, cfg.connections, false)
        } else {
            let b = single_stream(&client, url, &part, total, 0, on_progress).await?;
            (b, 1, false)
        }
    };

    tokio::fs::rename(&part, dest).await?;
    Ok(Outcome { path: dest.to_path_buf(), bytes, connections, resumed })
}

/// Probe with a 1-byte ranged GET — more reliable than HEAD across servers/CDNs.
/// A `206 Partial Content` proves range support and its `Content-Range` header
/// carries the true total size (`bytes 0-0/<total>`).
async fn probe(client: &reqwest::Client, url: &str) -> (Option<u64>, bool) {
    let resp = match client.get(url).header(reqwest::header::RANGE, "bytes=0-0").send().await {
        Ok(r) => r,
        Err(_) => return (None, false),
    };
    if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        let total = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.rsplit('/').next())
            .and_then(|s| s.trim().parse::<u64>().ok());
        (total, true)
    } else if resp.status().is_success() {
        (resp.content_length(), false)
    } else {
        (None, false)
    }
}

fn spawn_monitor(
    downloaded: Arc<AtomicU64>,
    done: Arc<AtomicBool>,
    total: Option<u64>,
    on_progress: Arc<dyn Fn(Progress) + Send + Sync>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while !done.load(Ordering::Relaxed) {
            on_progress(Progress { downloaded: downloaded.load(Ordering::Relaxed), total });
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        on_progress(Progress { downloaded: downloaded.load(Ordering::Relaxed), total });
    })
}

async fn segmented_download(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    connections: usize,
    total: u64,
    on_progress: Arc<dyn Fn(Progress) + Send + Sync>,
) -> Result<()> {
    // Pre-allocate so each segment can write at its offset.
    let file = tokio::fs::OpenOptions::new().create(true).write(true).truncate(true).open(part).await?;
    file.set_len(total).await?;
    drop(file);

    let ranges = split_ranges(total, connections);
    let downloaded = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let monitor = spawn_monitor(downloaded.clone(), done.clone(), Some(total), on_progress);

    let mut handles = Vec::new();
    for (start, end) in ranges {
        let client = client.clone();
        let url = url.to_string();
        let part = part.to_path_buf();
        let counter = downloaded.clone();
        handles.push(tokio::spawn(async move {
            download_range(&client, &url, &part, start, end, counter).await
        }));
    }

    let mut result = Ok(());
    for h in handles {
        match h.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => result = Err(e),
            Err(_) => result = Err(TransferError::Join),
        }
    }
    done.store(true, Ordering::Relaxed);
    let _ = monitor.await;
    result
}

async fn download_range(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    start: u64,
    end: u64,
    counter: Arc<AtomicU64>,
) -> Result<()> {
    let mut resp = client
        .get(url)
        .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
        .send()
        .await?
        .error_for_status()?;

    let mut file = tokio::fs::OpenOptions::new().write(true).open(part).await?;
    file.seek(std::io::SeekFrom::Start(start)).await?;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
        counter.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    file.flush().await?;
    Ok(())
}

/// Single-stream download to `part`, optionally resuming from `start` bytes.
async fn single_stream(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    total: Option<u64>,
    start: u64,
    on_progress: Arc<dyn Fn(Progress) + Send + Sync>,
) -> Result<u64> {
    let downloaded = Arc::new(AtomicU64::new(start));
    let done = Arc::new(AtomicBool::new(false));
    let monitor = spawn_monitor(downloaded.clone(), done.clone(), total, on_progress);

    let mut req = client.get(url);
    let mut file = if start > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={start}-"));
        tokio::fs::OpenOptions::new().write(true).append(true).open(part).await?
    } else {
        tokio::fs::File::create(part).await?
    };

    let mut resp = req.send().await?.error_for_status()?;
    let mut bytes = start;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
        bytes += chunk.len() as u64;
        downloaded.store(bytes, Ordering::Relaxed);
    }
    file.flush().await?;

    done.store(true, Ordering::Relaxed);
    let _ = monitor.await;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_cover_the_whole_file_without_gaps() {
        let total = 1000;
        let r = split_ranges(total, 4);
        assert_eq!(r.len(), 4);
        assert_eq!(r[0].0, 0);
        assert_eq!(r.last().unwrap().1, total - 1);
        for w in r.windows(2) {
            assert_eq!(w[0].1 + 1, w[1].0);
        }
        let sum: u64 = r.iter().map(|(s, e)| e - s + 1).sum();
        assert_eq!(sum, total);
    }

    #[test]
    fn handles_indivisible_and_tiny() {
        let sum = |r: &[(u64, u64)]| r.iter().map(|(s, e)| e - s + 1).sum::<u64>();
        assert_eq!(sum(&split_ranges(10, 3)), 10);
        let tiny = split_ranges(1, 8);
        assert!(!tiny.is_empty() && tiny.len() <= 8);
        assert_eq!(sum(&tiny), 1);
        assert_eq!(split_ranges(0, 4), vec![(0, 0)]);
    }

    #[test]
    fn part_path_appends_suffix() {
        assert_eq!(part_path(Path::new("a/b/ubuntu.iso")), PathBuf::from("a/b/ubuntu.iso.part"));
        assert_eq!(part_path(Path::new("f")), PathBuf::from("f.part"));
    }
}
