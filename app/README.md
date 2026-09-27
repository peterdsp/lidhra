# Lidhra desktop (Tauri)

The native desktop shell. It loads the **same** `../ui/index.html` the server serves,
but drives the `lidhra-debrid` + `lidhra-transfer` crates directly through Tauri
commands (no HTTP) - the UI auto-detects the Tauri window and uses `invoke`.

## Cargo features

| Feature | Default | What it does |
|---------|---------|--------------|
| `kofi` | on | Direct-download flavor: 7-day trial, license key, self-updater. |
| `appstore` | off | App Store flavor: paid upfront, no trial, no updater. CI swaps `kofi` for it. |
| `p2p` | on | On-device BitTorrent (`lidhra-torrent`): magnets download directly when no debrid account is connected. Independent of the flavor. Remove it from `default` for a debrid-only binary; the UI hides every P2P element when the engine is absent. |

## iOS notes for direct (P2P) downloads

- Files land in the app's Documents folder, so they appear in Files under "On My iPhone > Lidhra" like HTTPS downloads.
- The Swift plugin (`plugins/native/ios`) reports app state, network path and Low Power Mode to Rust through the exported C function `lidhra_native_event`. On `didEnterBackground` it asks for the extra background seconds iOS grants; when they run out the engine pauses and saves. No background mode is declared or claimed: a download only runs while the app is open.
- Wi-Fi only is on by default; Local Service Discovery is off so the Local Network prompt never appears.
- Session file, settings and the DHT table live under the app's data directory (`torrents/`), next to the remembered provider login.

## Desktop polish (tray, live progress)

- **Tray / menu bar** (`src/tray.rs`, desktop only): a status item with the brand
  mark (a monochrome template on macOS, the coloured app icon on Windows and
  Linux). Tooltip and the first menu line show the live totals ("2 downloads · 46%",
  "3 done · 1 failed", "Idle"); on macOS the percentage also sits beside the icon
  while something is downloading. Menu: Show Lidhra, Open Downloads Folder, Quit.
  Left-click brings the window back.
- **Close to tray**: closing the window hides it and downloads keep running.
  Quit from the tray menu, the app menu or Cmd+Q. On macOS clicking the Dock
  icon reopens the window.
- **Live download events**: the download engine's progress callback emits
  `download-progress` (rate-limited to 4/s per download) and `download-done`
  Tauri events whose payload is one `downloads` row. The web UI listens when it
  runs inside Tauri and moves the list live, keeping the `downloads` poll as a
  slow safety net (6s instead of 1.5s). The browser build still polls.
- The tray summary is refreshed by a 1s task that only touches the OS when the
  numbers change.

## Run

```sh
# one-time: install the Tauri CLI
cargo install tauri-cli --version "^2"

# from repo root
cargo tauri dev        # or: cd app/src-tauri && cargo run
```

Requires a system webview (WebKit on macOS, WebView2 on Windows, WebKitGTK on Linux),
which is why this crate is intentionally **outside** the `crates/` CI workspace.

## Build installers

```sh
cargo tauri build      # .dmg / .app / .msi / .AppImage / .deb per platform
cargo tauri icon design/Brand/png/lidhra-app-icon-filled-1024.png   # regenerate full icon set
```

Status: scaffold - commands mirror the verified `lidhra-server` API.
