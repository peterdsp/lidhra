# Lidhra on TVs

On a TV Lidhra is a **library + player**: connect a debrid provider, browse the
cloud transfers, pick a file, play it. Nothing is downloaded to the TV. Sign-in
uses a short code approved on a phone (Real-Debrid, AllDebrid) or a pasted API key.

## Platforms

| Platform | How | Build | Status (2026-09-27) |
| --- | --- | --- | --- |
| Samsung Tizen TV | Shared web UI in TV mode + wasm bridge, `.wgt` widget | `scripts/tv/build.sh tizen` | Builds an **unsigned** `.wgt`. Signing needs Tizen Studio and a Samsung certificate profile (`TIZEN_PROFILE=...`). Not run on a TV. |
| LG webOS TV | Same bundle, `.ipk` | `scripts/tv/build.sh webos` | Builds `dev.peterdsp.lidhra_<version>_all.ipk` with LG's `ares-package` (via npx). Not run on a TV. |
| HarmonyOS (Huawei Vision, tablets) | ArkWeb shell (`tv/harmonyos`) over the same bundle | `scripts/tv/build.sh harmonyos`, then DevEco Studio | Project + bundle ready; not built (DevEco / hvigor not installed here). |
| TV browsers (any) | The same bundle served as a static page | `scripts/tv/build.sh web`, host `build/tv/web/` | Verified in a 1920x1080 browser: providers from wasm, Real-Debrid device code, D-pad focus, library, files, playback, Back. |
| Android TV, Google TV, Fire TV | Tauri app with `tauri.tv.conf.json` (opens `?tv=1`), leanback manifest | `scripts/tv/android-tv.sh build` | APK built and run on the Android TV emulator, launcher banner verified. See [android-tv.md](android-tv.md). |
| Apple TV | Native SwiftUI app over the Rust core (C ABI), tvOS 17+ | `scripts/tv/build-apple-tv-core.sh`, then Xcode | Builds for simulator and device; runs in the Apple TV 4K simulator with providers from the Rust core and a live Real-Debrid code + QR. Library, Files and Player not exercised (no test account). See [apps/tvos/README.md](../../apps/tvos/README.md). |
| Roku, Vizio, older Panasonic, any DLNA TV | Cast from the desktop app (file sheet: Cast to TV) | `crates/lidhra-cast` | Discovery verified against a real Samsung Q70 on the LAN; play/stop not exercised on hardware. |

## The TV mode (web)

`ui/index.html` switches into a 10-foot layout when the URL has `?tv=1`, the user
agent is a TV (Tizen, webOS, SMART-TV, HbbTV, Fire TV, Android TV, BRAVIA, VIDAA),
or a packaged build sets `window.LIDHRA_TV`. `?tv=0` forces it off.

- **Screens**: Connect (provider grid), Sign in (code + URL, polls until approved),
  API key, Library (ready titles first), Files, and the existing player.
- **Remote**: arrows move focus to the nearest control in that direction, Enter
  activates, Back goes back and exits the app from the first screen (Tizen 10009,
  webOS 461, Escape, Backspace outside a text field). In the player, left/right
  seek, Enter or Play/Pause toggles. Tizen media keys are registered.
- **Backend**: `api()` uses the wasm bridge when `window.LIDHRA_WEB_BRIDGE` is set
  (packaged web TVs), Tauri commands on Android TV, or `lidhra-server` in a browser.
- **Old engines**: the TV code avoids optional chaining, nullish coalescing,
  `color-mix`, `backdrop-filter` and `inset`. Target: Tizen 5.5+, webOS 5+,
  Android TV WebView. WebAssembly is required.
- **Languages**: all TV strings are translated into the seven UI languages.

## Packaged web bundle

`scripts/tv/build-web.sh` compiles `crates/lidhra-bridge` to wasm (size-optimised,
about 945 KiB), generates `no-modules` bindings, inlines the wasm as base64 (TV
runtimes load from `file://` or `app://`, where `fetch` of a local `.wasm` often
fails), and injects `bridge/lidhra_bridge.js`, `bridge/lidhra_bridge_wasm.js` and
`tv/web/tv-boot.js` into the `<head>` of a copy of `ui/index.html`. Output:
`build/tv/web/` (gitignored), about 1.7 MB.

## What needs you

- **Samsung**: a Samsung developer account and certificate (Tizen Studio Certificate
  Manager), then `TIZEN_PROFILE=<name> scripts/tv/build.sh tizen`. Seller Office for the store.
- **LG**: a TV in Developer Mode (`ares-setup-device`) to install; LG Seller Lounge to publish.
- **HarmonyOS**: DevEco Studio, a Huawei developer account and signing config; check
  that the SDK accepts the `tv` device type in `entry/src/main/module.json5`.
- **Google Play / Amazon**: a release keystore (see android-tv.md) and store listings.
- **Apple TV**: signing and TestFlight through App Store Connect.
- **Casting**: try it on your own TV from the desktop app (file sheet, Cast to TV), or
  `cargo run -p lidhra-cast --example cast -- discover` then `-- play <n> <url>`.
