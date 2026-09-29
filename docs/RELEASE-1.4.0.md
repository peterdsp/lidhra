# Lidhra 1.4.0 (iOS build 11)

Marketing version 1.3.0 to 1.4.0; `bundle.iOS.bundleVersion` from 10 to 11, both in
`app/src-tauri/tauri.conf.json`. The Apple TV and HarmonyOS projects follow 1.4.0.

## App Store release notes (What's New)

More services, and a better desktop companion.

- Five more debrid services: Debrid-Link, Offcloud, Mega-Debrid, Deepbrid and High-Way.
- Sign in to Real-Debrid and AllDebrid with a short code approved on another device.

## What changed

- **Providers**: adapters for Debrid-Link, Offcloud, Mega-Debrid (login:password with
  token renewal), Deepbrid and High-Way, listed everywhere through the registry.
- **Device login** (`lidhra_debrid::device_auth`): Real-Debrid OAuth device code with
  token refresh, AllDebrid PIN.
- **Desktop**: tray / menu-bar item with live download totals (the percentage sits next
  to the icon on macOS), close to tray, Dock click reopens. Downloads push live progress
  events to the UI instead of relying on a fast poll.
- **Cast to TV** (desktop): the file sheet finds DLNA renderers and Roku devices on the
  LAN and plays a cloud link on them (`crates/lidhra-cast`). macOS asks for Local
  Network access the first time.
- **TVs**: a D-pad TV mode in the shared UI and packages for Samsung Tizen, LG webOS,
  HarmonyOS, TV browsers, Android / Google / Fire TV and Apple TV (see `docs/tv/`).

## Release packages

| Package | Notes |
| --- | --- |
| Windows, Linux installers + `latest.json` | Built and signed by `release.yml`; the in-app updater reads `latest.json`. |
| macOS | Not attached: the `APPLE_CERTIFICATE` secret holds an Apple Development certificate, which does not match the signing identity; a Developer ID Application `.p12` is needed (same gap as 1.3.0). |
| iOS | Uploaded to App Store Connect by `ios-appstore.yml`. |
| `Lidhra-1.4.0-android-tv-arm64.apk` | Android / Google / Fire TV. Signed with a debug key for sideloading; a store build needs the release keystore (`docs/tv/android-tv.md`). |
| `dev.peterdsp.lidhra_1.4.0_all.ipk` | LG webOS, for TVs in Developer Mode. |
| `Lidhra-1.4.0-tizen-unsigned.wgt` | Samsung Tizen. Must be signed with a Samsung certificate before a TV accepts it. |
| `Lidhra-1.4.0-tv-web.zip` | Self-contained web-TV bundle for any TV browser or static host. |
| `Lidhra-1.4.0-harmonyos-project.zip` | DevEco Studio project with the bundle in place; build and sign in DevEco. |
| Apple TV | Not attached: needs App Store signing (`apps/tvos`). |

## Verification

- Rust workspace tests, clippy on the new crates (native and wasm), UI tests.
- TV mode driven by keyboard in a 1920x1080 browser; Android TV APK on the Android TV
  emulator; Apple TV app in the tvOS simulator; cast discovery against a real Samsung.
- Not verified: installs on physical Tizen / webOS / HarmonyOS TVs, provider flows with a
  real account on TV, and cast playback on hardware.
