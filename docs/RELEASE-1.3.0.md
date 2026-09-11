# Lidhra 1.3.0 (iOS build 9)

Marketing version 1.2.0 to 1.3.0; `bundle.iOS.bundleVersion` from 8 to 9. Both
set in `app/src-tauri/tauri.conf.json`, the source `cargo tauri ios init`
regenerates the Xcode project from. Minimum iOS stays 15.0. The app Cargo
package version is reconciled from 1.0.0 to 1.3.0.

## App Store release notes (What's New)

A layout that follows the space you give it.

- On a wide window or a large display, the transfer list and the selected
  file's details now sit side by side, so you can browse and act without a
  sheet in the way.
- On phones the familiar single-pane layout is unchanged: tap a transfer to open
  its files and actions in the sheet.
- Share and Open In now point at the button you tapped instead of the bottom of
  the screen.
- Groundwork for iPhone Duo: the interface reshapes itself around available
  regions. Full iPhone Duo support arrives once the required Xcode 27.1 tools
  ship.

## What changed

- Shared web UI (`ui/index.html`): a content-driven adaptive layer
  (`LidhraAdaptive`) chooses a compact, regular, split, or stack layout from
  available space and any reported display division. One reusable selected-file
  detail surface renders in the modal sheet (single pane) or inline beside /
  below the list (two panes), preserving selection and list position across
  every transition. `100vh` / `100dvh` ordering fixed so the viewport tracks the
  keyboard. Empty-detail string added in all seven languages; a latent gap where
  the Simplified Chinese direct-download strings never merged is fixed.
- Native geometry bridge (`NativePlugin.swift`): a versioned snapshot of scene
  bounds, the webview frame, safe-area insets, keyboard overlap, and active
  reserved regions is pushed to `window.__lidhraGeometry`, coalesced and
  revision-guarded. Observers are torn down with their owner.
- Share / Open In popovers anchor to the initiating control; the anchor is
  plumbed through the Rust bridge (`file_share` / `file_open_in`).
- iPhone Duo reserved-region emission and the native player arrangement are
  isolated behind the `LIDHRA_DUO` compile flag (off in every current build),
  so the iOS 26 SDK build is unchanged. See `docs/adaptive-layout.md`.

## Verified

- `node --test test/adaptive.test.mjs`: 15/15.
- App crate `cargo check` (default and App Store feature sets).
- `crates` workspace `cargo test` and `cargo clippy -D warnings`.
- Browser: compact / regular / split by width, tap-to-select, continuity across
  resizes, and a synthetic central-fold split (labelled a simulation).

## Not yet verified (blocked on tooling)

- Anything requiring Xcode 27.1 and the iPhone Duo runtime: the reserved-region
  emitter, the native player arrangement, and every on-device Duo pose. 1.3.0 is
  Duo-*ready*, not Duo-*supported*, until those pass. Do not label it supported
  or ship it as such until then.

## Ship it

Same flow as before (not part of this change set):

```sh
./scripts/upload-appstore-ios.sh
```

or run `ios-appstore.yml`. The workflow flips `default = ["kofi", "p2p"]` to
`["appstore", "p2p"]` and strips the Ko-fi UI; P2P stays on in the store binary.
