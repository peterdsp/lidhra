//! The engine's own session file.
//!
//! librqbit can persist a session itself, but it stores absolute output paths
//! and iOS moves the app container between installs and updates. Lidhra keeps
//! a small list of what was added (magnet, resolved `.torrent` bytes, selected
//! files, paused flag) and re-adds everything on launch relative to the current
//! download folder; librqbit then re-checks the pieces already on disk.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::Settings;

pub const TORRENTS_FILE: &str = "torrents.json";
pub const SETTINGS_FILE: &str = "p2p-settings.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTorrent {
    /// Lowercase hex v1 info hash.
    pub id: String,
    pub magnet: String,
    #[serde(default)]
    pub name: Option<String>,
    /// The resolved `.torrent` (base64) once metadata is known, so a restart
    /// does not need the DHT again.
    #[serde(default)]
    pub torrent_b64: Option<String>,
    #[serde(default)]
    pub only_files: Option<Vec<usize>>,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub added_at: u64,
}

impl SavedTorrent {
    pub fn torrent_bytes(&self) -> Option<Vec<u8>> {
        let b = self.torrent_b64.as_deref()?;
        base64::engine::general_purpose::STANDARD.decode(b).ok()
    }

    pub fn set_torrent_bytes(&mut self, bytes: Option<&[u8]>) {
        self.torrent_b64 = bytes.map(|b| base64::engine::general_purpose::STANDARD.encode(b));
    }
}

pub fn load_torrents(path: &Path) -> Vec<SavedTorrent> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Write the list atomically (temp file + rename) so a crash mid-write never
/// leaves a truncated session behind.
pub fn save_torrents(path: &Path, list: &[SavedTorrent]) -> std::io::Result<()> {
    write_atomic(path, &serde_json::to_vec_pretty(list)?)
}

pub fn load_settings(path: &Path) -> Option<Settings> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save_settings(path: &Path, s: &Settings) -> std::io::Result<()> {
    write_atomic(path, &serde_json::to_vec_pretty(s)?)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("lidhra-torrent-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn torrents_round_trip() {
        let dir = tmp_dir("persist");
        let path = dir.join(TORRENTS_FILE);
        assert!(load_torrents(&path).is_empty(), "missing file reads as empty");

        let mut a = SavedTorrent {
            id: "a".repeat(40),
            magnet: format!("magnet:?xt=urn:btih:{}&dn=one", "a".repeat(40)),
            name: Some("one".into()),
            torrent_b64: None,
            only_files: Some(vec![0, 2]),
            paused: true,
            added_at: 1_700_000_000,
        };
        a.set_torrent_bytes(Some(b"d4:infod4:name3:onee"));
        let b = SavedTorrent {
            id: "b".repeat(40),
            magnet: format!("magnet:?xt=urn:btih:{}", "b".repeat(40)),
            name: None,
            torrent_b64: None,
            only_files: None,
            paused: false,
            added_at: 0,
        };
        save_torrents(&path, &[a.clone(), b.clone()]).unwrap();
        assert!(!dir.join("torrents.json.tmp").exists(), "temp file is renamed away");

        let back = load_torrents(&path);
        assert_eq!(back, vec![a.clone(), b]);
        assert_eq!(back[0].torrent_bytes().as_deref(), Some(&b"d4:infod4:name3:onee"[..]));
        assert_eq!(back[1].torrent_bytes(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_reads_as_empty() {
        let dir = tmp_dir("corrupt");
        let path = dir.join(TORRENTS_FILE);
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load_torrents(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_round_trip_and_defaults() {
        let dir = tmp_dir("settings");
        let path = dir.join(SETTINGS_FILE);
        assert_eq!(load_settings(&path), None);
        let s = Settings { seed: true, wifi_only: false, max_connections: 20 };
        save_settings(&path, &s).unwrap();
        assert_eq!(load_settings(&path), Some(s));
        // Older files with missing keys fall back to the defaults.
        std::fs::write(&path, b"{\"seed\":true}").unwrap();
        assert_eq!(load_settings(&path), Some(Settings { seed: true, ..Settings::default() }));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
