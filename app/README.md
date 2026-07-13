# Lidhra desktop (Tauri)

The native desktop shell. It loads the **same** `../ui/index.html` the server serves,
but drives the `lidhra-debrid` + `lidhra-transfer` crates directly through Tauri
commands (no HTTP) — the UI auto-detects the Tauri window and uses `invoke`.

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

Status: scaffold — commands mirror the verified `lidhra-server` API.
