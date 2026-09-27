# Lidhra for Apple TV

A native SwiftUI tvOS app. On a TV, Lidhra is a library and a player (stream your cloud), not a
downloader: connect a debrid provider, browse the transfers already in your cloud, pick a file,
play it. Nothing is downloaded to the Apple TV.

tvOS has no `WKWebView`, so unlike the other platforms this app does not reuse `ui/index.html`.
All provider logic stays in Rust (`crates/lidhra-bridge` over `crates/lidhra-debrid`); Swift only
draws screens and plays video.

## Requirements

- Xcode 26 or newer with the tvOS SDK and a tvOS simulator runtime (developed with Xcode 27.1, tvOS 27).
- Rust via rustup, with `aarch64-apple-tvos` and `aarch64-apple-tvos-sim` (the build script adds
  them if missing). Both are tier 2 targets with a prebuilt std, no nightly needed.
- Python 3 with Pillow, only to regenerate the brand assets.

## Build

```sh
# 1. Build the Rust core into apps/tvos/LidhraCore.xcframework (gitignored, rebuild after Rust changes)
scripts/tv/build-apple-tv-core.sh

# 2. Build the app
open apps/tvos/Lidhra.xcodeproj          # then run the "Lidhra" scheme on an Apple TV simulator
# or from the command line:
xcodebuild -project apps/tvos/Lidhra.xcodeproj -scheme Lidhra -configuration Debug \
  -sdk appletvsimulator -destination 'platform=tvOS Simulator,name=Apple TV 4K (3rd generation)' build
```

The script builds `lidhra-bridge` as a release staticlib for both targets with
`TVOS_DEPLOYMENT_TARGET=17.0` (the same minimum as the app, so `ring`'s C objects and the Rust
objects agree) and points `cc` at the matching Xcode SDK. `PROFILE=debug` builds the debug profile.

### Run in the simulator from the command line

```sh
UDID=$(xcrun simctl list devices available | awk -F '[()]' '/Apple TV 4K \(3rd generation\) \(/{print $2; exit}')
xcrun simctl boot "$UDID"; open -a Simulator
xcrun simctl install "$UDID" <DerivedData>/Build/Products/Debug-appletvsimulator/Lidhra.app
xcrun simctl launch "$UDID" dev.peterdsp.lidhra
xcrun simctl io "$UDID" screenshot connect.png
```

In the Simulator window the arrow keys, Return (select) and Escape (Menu) drive the remote.
Debug builds also accept `-LidhraOpenProvider Real-Debrid` as a launch argument
(`xcrun simctl launch "$UDID" dev.peterdsp.lidhra -LidhraOpenProvider Real-Debrid`), which opens
that provider's sign-in screen directly. The scheme carries it, disabled.

## Architecture

```
SwiftUI views (Lidhra/Views)
   |  await model.core.call("transfers")
LidhraCore wrapper (Lidhra/Core/LidhraCore.swift)   JSON in, JSON out, async/await
   |  lidhra_bridge_call(name, args_json, ctx, callback)
C ABI (crates/lidhra-bridge/include/lidhra_bridge.h, module LidhraBridge)
   |
lidhra-bridge (Rust, tokio runtime)   command table, session + OAuth refresh
   |
lidhra-debrid (Rust)   provider adapters, device login (Real-Debrid OAuth, AllDebrid PIN)
```

- `LidhraCore.call(_:_:)` wraps one `lidhra_bridge_call` in `withCheckedThrowingContinuation`.
  Each call passes a retained `PendingCall` box as `ctx`; the `@convention(c)` callback copies the
  JSON, releases the box and resumes the continuation. A lock plus a 120 s timeout make sure the
  continuation is resumed exactly once even if a callback never arrives. `{"error": ...}` payloads
  become `LidhraError.bridge`. The wrapper is `@MainActor`, so results land on the main actor.
- `AppModel` owns the sign-in phase. The `session` JSON from `connect`, `login_poll` or `restore`
  is stored in the Keychain (`kSecClassGenericPassword`, service `dev.peterdsp.lidhra.tv`). On
  launch the app calls `restore {session}` and saves the session it gets back (a Real-Debrid
  session may carry a refreshed token).
- Screens: Connect (provider grid), Device login (code, URL, QR, polling), Token entry (tvOS
  keyboard), Library (Ready first, refresh on appear and every 10 s while visible), Files, Player
  (`AVPlayerViewController` with title metadata), Settings (account, sign out).
- User-facing strings live in `Lidhra/Support/Strings.swift`, all through `String(localized:)`.
- The Xcode project is hand-written and uses a synchronized folder (`Lidhra/`): new Swift files
  and assets dropped in that folder are part of the target without editing `project.pbxproj`.

### Commands used

| Command | Where |
| --- | --- |
| `providers` | Connect: provider grid (`device_login` picks code vs API key) |
| `login_start {provider}` | Device login: code, `verify_url`, `interval`, `expires_in` |
| `login_poll {provider, handle}` | Device login: every `interval` s until not pending or expired |
| `connect {provider, token}` | Token entry |
| `restore {session}` | Launch |
| `transfers` | Library |
| `links {id}` | Files of a Ready transfer |
| `disconnect` | Settings: sign out |

## Assets

`Lidhra/Assets.xcassets` holds the layered "App Icon & Top Shelf Image" brand assets (App Icon
400x240 @1x/@2x and App Store 1280x768, each a Front mark layer over a Back brand plate; Top Shelf
1920x720 and Top Shelf Wide 2320x720 at @1x/@2x), the in-app mark, the accent colour (#18D88F)
and the launch colour. They are generated from the brand geometry in `design/Brand`:

```sh
python3 apps/tvos/tools/generate_brand_assets.py
```

## Bundle id and Universal Purchase

The bundle id is `dev.peterdsp.lidhra`, the same as the iOS app. That makes the tvOS app a new
platform of the same App Store Connect record (Universal Purchase): one purchase covers iPhone,
iPad and Apple TV. The marketing version tracks the iOS app (1.3.0); the build number
(`CURRENT_PROJECT_VERSION`) is counted separately for the tvOS platform.

## Not done yet

- App Store signing, archive/export and TestFlight upload (no CI workflow; signing will need the
  manual distribution setup the iOS app uses, with a tvOS App Store profile).
- Top Shelf extension (only the static Top Shelf images exist).
- Localisation (English only; strings are centralised for a String Catalog).
- Formats AVFoundation cannot play (MKV, AVI, ...) would need a custom player; those files are
  marked "May not play on Apple TV" and a playback failure is reported.
- The Library, Files and Player screens have not been exercised against a real account (no test
  account); only the Connect and device-login screens were verified in the simulator.
