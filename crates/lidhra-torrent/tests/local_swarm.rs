//! End to end on the loopback interface: a librqbit seeder serves a generated
//! file, the engine downloads it from a magnet with the seeder as the only
//! peer, finishes, pauses (seeding is off by default), and comes back as Done
//! after a restart. No DHT, no trackers, nothing leaves 127.0.0.1.
//!
//! Set LIDHRA_SKIP_SWARM_TEST=1 to skip (sandboxes without loopback sockets).

use lidhra_torrent::{Engine, EngineConfig, State, TORRENTS_FILE};
use librqbit::{CreateTorrentOptions, ListenerOptions, Session, SessionOptions};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("lidhra-swarm-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn payload(len: usize) -> Vec<u8> {
    // Deterministic pseudo-random bytes so pieces are not trivially compressible.
    let mut x: u32 = 0x9e37_79b9;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

async fn wait_for(engine: &Engine, id: &str, want: State, timeout: Duration) {
    let start = Instant::now();
    loop {
        let t = engine.get(id).await.expect("torrent is listed");
        if t.state == want {
            return;
        }
        assert!(t.state != State::Error, "torrent errored: {:?}", t.error);
        assert!(start.elapsed() < timeout, "timed out waiting for {want:?}, last seen {t:?}");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_from_a_local_seeder_and_survives_a_restart() {
    if std::env::var_os("LIDHRA_SKIP_SWARM_TEST").is_some() {
        eprintln!("skipped: LIDHRA_SKIP_SWARM_TEST is set");
        return;
    }
    let root = scratch("e2e");
    let seed_dir = root.join("seed");
    std::fs::create_dir_all(&seed_dir).unwrap();
    let data = payload(3 * 1024 * 1024 + 12_345);
    std::fs::write(seed_dir.join("payload.bin"), &data).unwrap();

    // The seeder: plain librqbit, listening on loopback only.
    let seeder = Session::new_with_opts(
        seed_dir.clone(),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            listen: Some(ListenerOptions { listen_addr: "127.0.0.1:0".parse().unwrap(), ..Default::default() }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (created, _seed_handle) = seeder
        .create_and_serve_torrent(
            &seed_dir.join("payload.bin"),
            CreateTorrentOptions { name: Some("payload.bin"), trackers: Vec::new(), piece_length: Some(64 * 1024) },
        )
        .await
        .unwrap();
    let seeder_addr: SocketAddr = seeder.listen_addr().expect("seeder listens");
    let magnet = format!("magnet:?xt=urn:btih:{}&dn=payload.bin", created.info_hash().as_string());

    // The engine under test.
    let out_dir = root.join("out");
    let state_dir = root.join("state");
    let mut cfg = EngineConfig::new(&out_dir, &state_dir);
    cfg.dht = false;
    cfg.trackers = false;
    cfg.listen_addr = Some("127.0.0.1:0".parse().unwrap());
    let engine = Engine::start(cfg).await.unwrap();

    let t = engine.add_magnet_with_peers(&magnet, vec![seeder_addr]).await.unwrap();
    assert_eq!(t.state, State::Resolving);
    assert_eq!(t.name, "payload.bin", "the dn shows before metadata arrives");
    let id = t.id.clone();
    assert_eq!(engine.list().await.len(), 1);
    // Adding the same magnet twice is a no-op.
    assert_eq!(engine.add_magnet(&magnet).await.unwrap().id, id);
    assert_eq!(engine.list().await.len(), 1);

    wait_for(&engine, &id, State::Done, Duration::from_secs(90)).await;

    let t = engine.get(&id).await.unwrap();
    assert_eq!(t.total, data.len() as u64);
    assert_eq!(t.downloaded, data.len() as u64);
    assert!((t.progress - 1.0).abs() < f32::EPSILON);
    let files = engine.files(&id).await.unwrap();
    assert_eq!(files.len(), 1);
    assert!(files[0].done && files[0].included);
    assert_eq!(files[0].size, data.len() as u64);
    assert_eq!(std::fs::read(&files[0].path).unwrap(), data, "bytes are identical");
    assert!(files[0].path.starts_with(&out_dir));

    let saved = std::fs::read_to_string(state_dir.join(TORRENTS_FILE)).unwrap();
    assert!(saved.contains(&id), "session file lists the torrent");
    assert!(saved.contains("torrent_b64\": \""), "resolved metadata is kept for the next launch");

    engine.shutdown().await;
    drop(engine);

    // Second launch: restored from the session file, hash-checked, still Done.
    let mut cfg = EngineConfig::new(&out_dir, &state_dir);
    cfg.dht = false;
    cfg.trackers = false;
    cfg.listen_addr = Some("127.0.0.1:0".parse().unwrap());
    let engine = Engine::start(cfg).await.unwrap();
    assert_eq!(engine.list().await.len(), 1, "restored on launch");
    wait_for(&engine, &id, State::Done, Duration::from_secs(30)).await;
    let t = engine.get(&id).await.unwrap();
    assert_eq!(t.downloaded, data.len() as u64);

    // Pause and resume are honoured and persisted.
    engine.pause(&id).await.unwrap();
    let saved = std::fs::read_to_string(state_dir.join(TORRENTS_FILE)).unwrap();
    assert!(saved.contains("\"paused\": true"));

    // Remove with files cleans the download folder.
    engine.remove(&id, true).await.unwrap();
    assert!(engine.list().await.is_empty());
    assert!(!files[0].path.exists(), "file deleted on request");
    engine.shutdown().await;
    seeder.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}
