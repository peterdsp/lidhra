//! The session and settings files must never live where peers can write.

use lidhra_torrent::{Engine, EngineConfig, Error};

#[tokio::test]
async fn refuses_a_state_folder_inside_the_download_folder() {
    let root = std::env::temp_dir().join(format!("lidhra-statedir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let out = root.join("downloads");
    let cfg = EngineConfig::new(&out, out.join("torrents"));
    match Engine::start(cfg).await {
        Err(Error::StateInsideDownloads(p)) => assert_eq!(p, out.join("torrents")),
        other => panic!("expected StateInsideDownloads, got {:?}", other.map(|_| ())),
    }
    let _ = std::fs::remove_dir_all(&root);
}
