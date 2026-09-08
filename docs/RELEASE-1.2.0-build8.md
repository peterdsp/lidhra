# Lidhra 1.2.0 (iOS build 8)

Marketing version stays 1.2.0; `bundle.iOS.bundleVersion` goes from 7 to 8.

## App Store release notes (What's New)

Download magnets without a debrid account.

- Paste a magnet and tap Download: Lidhra now downloads it directly on your device. No account, no sign-up.
- Debrid stays the fast lane. With a provider connected, magnets still go to the cloud, and the add sheet offers "Download directly on this device" when you want it.
- Direct downloads run while Lidhra is open and pick up where they left off when you come back. Wi-Fi only is on by default; sharing finished downloads is off unless you turn it on.
- Files land where they always did: Files, On My iPhone, Lidhra.

## What changed

- New crate `crates/lidhra-torrent`: an on-device BitTorrent engine over `librqbit` (pure Rust, rustls, DHT, PEX, trackers, magnet metadata, per-file progress) with Lidhra's policy on top: seeding off by default with a ratio 1.0 cap when on, pause on cellular and while suspended, free-space check, a session file that survives restarts and app updates.
- `crates/vendor/librqbit-dualstack-sockets`: one cfg fix (`target_os = "macos"` to `target_vendor = "apple"`) so librqbit links on iOS. See `crates/vendor/README.md`.
- Tauri app: `p2p` Cargo feature (on by default, independent of `kofi` / `appstore`), new commands for local torrents, the `lidhra_native_event` C entry point for the Swift lifecycle bridge.
- Swift plugin: background task on `didEnterBackground`, pause and save when the time is up or on terminate, resume on foreground, `NWPathMonitor` for Wi-Fi only, Low Power Mode awareness.
- Shared web UI: add sheet routes magnets to the device when no provider is connected (explicit toggle when one is), Direct badge with peers, speed and ETA on cards, pause / resume / remove / delete-files and copy-magnet in the file sheet, a Direct downloads settings block, new strings in all seven languages.
- `lidhra-server` gets the same engine and routes, so the browser mode behaves identically.

## Known limits users will notice

- On iPhone and iPad a direct download only runs while the app is open, plus the few seconds iOS grants after it goes to the background. There is no background mode.
- Connection count changes apply to torrents added afterwards.
- Other peers can see the device's IP address during a direct download. The debrid path does not have this property.

## Ship it

```sh
# local, signed, uploads to App Store Connect
./scripts/upload-appstore-ios.sh
```

or push a `v1.2.0-build8` style tag / run `ios-appstore.yml` from the Actions tab. The
workflow flips `default = ["kofi", "p2p"]` to `default = ["appstore", "p2p"]` and strips the
Ko-fi UI; P2P stays on in the store binary. If App Review pushes back on the engine,
change that sed to `default = ["appstore"]` and rebuild: the UI hides every P2P element
when the engine is absent.
