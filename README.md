<div align="center">
  <img src="docs/favicon.png" width="84" alt="Lidhra app logo">
  <h1>Lidhra</h1>
  <p><b>A fast, native, cross-platform download &amp; transfer app.</b><br>
  One Rust core, the interface each OS actually deserves. <em>Link what matters.</em></p>

  <a href="https://github.com/peterdsp/lidhra/actions/workflows/ci.yml"><img src="https://github.com/peterdsp/lidhra/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/license-MIT-2FD191" alt="MIT">
  <img src="https://img.shields.io/badge/rust-1.80%2B-15C3B6" alt="Rust">
  <a href="https://lidhra.peterdsp.dev"><img src="https://img.shields.io/badge/site-lidhra.peterdsp.dev-54E06A" alt="Website"></a>
</div>

---

> **Status: active.** The core libraries are built, tested, and usable from the CLI, the
> local server, and the Tauri app (desktop and iOS, the latter on the App Store). The shared
> web UI (`ui/index.html`) is the shipping interface; the TV shell is still design-only
> (see [`design/`](design/)). As of 1.3.0 the interface adapts to available display regions
> and is being readied for iPhone Duo (see [`docs/adaptive-layout.md`](docs/adaptive-layout.md)).
> This is a real, evolving codebase - not a finished product.

Lidhra is a reimagining of the classic torrent client as a **modern download manager** that:

- speaks each platform's **native design language** (Liquid Glass, Material 3, Fluent, libadwaita) from **one shared Rust core**;
- integrates every major **debrid service** (Real-Debrid, AllDebrid, TorBox, Premiumize, …) so the cloud does the torrenting and your device only ever does an **encrypted HTTPS download**;
- downloads a magnet **directly on the device** when no debrid account is connected (a built-in, pure Rust BitTorrent engine, seeding off by default);
- stays **tiny** (Tauri + system webview, no bundled Chromium) and **private** (no accounts, no telemetry).

## Why debrid, and what happens without it

The device hands a magnet to a debrid cloud service the user already pays for; that service torrents it on **its** servers and returns a direct **TLS-encrypted HTTPS link**. Lidhra is then just a resumable download manager - no P2P on the device, and usually much faster than a home connection.

Without a debrid account the same magnet downloads **directly on the device** through `lidhra-torrent` (DHT, PEX and trackers; magnet metadata; per-file progress). It is deliberately conservative: seeding is off by default, downloads pause on cellular data (Wi-Fi only is the default on iOS) and while the app is suspended, and there is a free-space check before anything is written. Peers can see the device's IP address, as with any BitTorrent client; the debrid path avoids that. With a provider connected the add sheet still offers "Download directly on this device" as an explicit choice.

On iOS a direct download only runs while Lidhra is open (plus the short grace period iOS grants when the app goes to the background); torrents resume from the pieces already on disk when the app is reopened.

## Repository layout

```
crates/          Rust workspace - the engine (builds & tested in CI)
  lidhra-debrid    unified interface over debrid providers + adapters + registry
  lidhra-transfer  segmented, resumable HTTPS download engine
  lidhra-torrent   on-device BitTorrent engine (magnets without a debrid account)
  vendor/          one patched upstream crate (see vendor/README.md)
  lidhra-cli       the `lidhra` binary: magnet -> debrid -> download
  lidhra-server    local HTTP server + JSON API (the headless / Web-UI mode)
ui/              the shared web UI - one page that runs in a browser (server) or a
                 native window (Tauri); auto-detects which and talks to the right backend
app/             the Tauri desktop app - native shell wrapping ui/ (compiles on macOS)
design/          the complete design system - brand, tokens, per-platform mockups,
                 the living styleguide (open design/index.html), developer color files
docs/            the marketing website (deployed to lidhra.peterdsp.dev via GitHub Pages)
```

## The crates

| Crate | What it does | State |
|-------|--------------|-------|
| **`lidhra-debrid`** | `DebridProvider` trait + adapters for **Real-Debrid** (live-tested), **AllDebrid**, **TorBox**, **Premiumize**, a provider **registry** (`build_provider`), and a cross-provider policy/failover engine. | builds + unit-tested |
| **`lidhra-transfer`** | Segmented, **resumable** HTTPS downloads: parallel Range connections, `.part` files with atomic rename, resume-from-partial. | verified byte-identical on live downloads |
| **`lidhra-torrent`** | On-device BitTorrent over `librqbit` (pure Rust, rustls): magnets without a debrid account, seeding policy, pause on cellular or suspend, free-space check, session file that survives restarts. Behind the `p2p` feature in the app and server. | builds for iOS + tested (loopback swarm) |
| **`lidhra-cli`** | `lidhra add "<magnet>" --provider <name>` - runs the whole pipeline end to end. | runs |
| **`lidhra-server`** | Serves the web UI + a JSON API over the engine - "Lidhra like qbittorrent-nox." | runs (verified) |
| **`app/` (Tauri)** | Native desktop shell wrapping the same UI; commands call the crates directly. | compiles on macOS |

## Quick start

```sh
# build & test everything
cd crates
cargo test --workspace

# run the CLI (needs a debrid account)
export DEBRID_TOKEN=your_api_token          # Real-Debrid / AllDebrid / TorBox / Premiumize
cargo run -p lidhra-cli -- add "magnet:?xt=urn:btih:...&dn=ubuntu-24.04.iso" \
    --provider realdebrid --out ~/Downloads
```

Use a **legitimate** magnet (a Linux ISO, a Creative-Commons film, your own file).

**Run the app** - a real UI you can click:

```sh
cargo run -p lidhra-server         # then open http://127.0.0.1:8787
# - or the native desktop window -
cargo install tauri-cli --version "^2" && cargo tauri dev
```

## Platform reach (honest)

Lidhra targets ~every screen, in three tiers:

- **Native app** - macOS, Windows, Linux, iOS/iPadOS, Android, Linux mobile, **Apple TV**, **Android/Google TV** (covers Sony, newer Panasonic, TCL, Hisense…), **Fire TV**.
- **Same web UI, re-packaged** - Samsung **Tizen**, LG **webOS**, **HarmonyOS** (web/ArkUI), and a **PWA** everywhere.
- **Cast-only** (no third-party app path) - **Vizio SmartCast, Roku, older Panasonic** → reached by casting a stream to them.

On a TV, Lidhra is a **library + player** (stream your cloud), not a downloader.

## Roadmap

- [x] `lidhra-debrid` - trait, adapters (RD / AllDebrid / TorBox / Premiumize), registry, policy
- [x] `lidhra-transfer` - segmented + resumable HTTPS engine
- [x] `lidhra-cli` - end-to-end pipeline
- [x] `lidhra-server` + `ui/` - runnable app (web UI + JSON API over the engine)
- [x] `app/` - Tauri desktop shell (compiles; wraps the shared UI)
- [ ] Desktop polish - tray / menu-bar surfaces, live download progress events
- [ ] `tv-mode` web UI (D-pad focus) → Tizen / webOS / HarmonyOS / browser TVs
- [ ] Native TV shells (Apple TV / Android TV) + `lidhra-cast`
- [ ] More provider adapters: Debrid-Link, Offcloud, Mega-Debrid, Deepbrid, High-Way

## License

MIT © Petros Dhespollari. See [`LICENSE`](LICENSE).

## Disclaimer

Independent project - **not affiliated with the qBittorrent project**. Lidhra is a content-neutral, general-purpose download tool.
