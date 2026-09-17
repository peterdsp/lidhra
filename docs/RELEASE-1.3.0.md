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
- Geometry contract v2 (`NativePlugin.swift` → `window.__lidhraGeometry`): the
  snapshot now carries a session id, scene id, size-class traits, layout margins,
  a structured keyboard state (docked / floating / overlap), and reserved regions
  as `{id, kind, active, rect}`. Acceptance is session-aware (a new attachment
  resets revision ordering; stale, replayed, and wrong-session events drop), and
  the web UI pulls the first snapshot on page-ready through a new `geometry_sync`
  command so no initial emission is lost to a startup race. The legacy v1 payload
  is still accepted. Converted geometry is recomputed from the retained raw
  snapshot on visual-viewport changes. Observers are torn down with their owner.
- Continuity: the selected-file detail moves between the inline pane and the
  modal sheet without a refetch, preserving the selected file, sub-line, and
  scroll anchor; presentation intent (active vs merely selected) decides whether
  a collapse to compact retains the detail; a removed selection clears explicitly
  instead of silently reselecting.
- Foldable browsers: `window.viewport.segments` (or `visualViewport.segments`)
  is normalised into the same region contract with a responsive fallback and no
  fabricated hinge.
- Share / Open In popovers anchor to the initiating control, mapped back to
  native points through the measured transform (`toNativeRect`); the anchor is
  plumbed through the Rust bridge (`file_share` / `file_open_in`).
- Reserved-region insets are mirrored to body-level overlays (sheets, player) so
  a presentation stays clear of a fold or occlusion; threshold hysteresis avoids
  mode oscillation while a fold still reacts immediately.
- iPhone Duo reserved-region emission and the native player arrangement are
  isolated behind the `LIDHRA_DUO` compile flag (off in every current build). The
  flag is now driven from the plugin's `Package.swift` from the environment, so
  it reaches the plugin compilation target and survives regeneration. See
  `docs/adaptive-layout.md`.

## Readiness

Using the brief's labels:

- **Adaptive groundwork: done.** The shared resizing, continuity, and browser
  fold handling work and are unit-tested.
- **Native integration validated: not yet.** The Swift plugin was syntax-parsed
  and the Rust bridge compiles, but no iOS compile/link or on-device native flow
  has run.
- **iPhone Duo support verified: no.** Blocked on Xcode 27.1 and the Duo runtime.
- **Release ready: no.** See below.

## Verified

- `node --test test/adaptive.test.mjs`: 23/23.
- App crate `cargo check --lib` (default and `appstore,p2p` feature sets).
- `crates` workspace `cargo build` / `cargo test` / `cargo clippy -D warnings`.
- `NativePlugin.swift` and `Package.swift` `swiftc -parse` (syntax only).
- In-app browser: compact / split by width, a synthetic v2 central-fold split
  with the gutter aligned to the fold, and a stale revision ignored (a
  simulation). Full matrix: `docs/verification/iphone-duo/`.

## Not yet verified (blocked on tooling)

- Anything requiring Xcode 27.1 and the iPhone Duo runtime: the reserved-region
  emitter, the native player arrangement, and every on-device Duo pose. 1.3.0 is
  Duo-*ready*, not Duo-*supported*, until those pass. Do not label it supported
  or ship it as such until then.
- A full local iOS compile/link of the updated plugin (`cargo tauri ios build`),
  which was not run here.

## Before shipping: build number

The checkout is 1.3.0 / bundle build 9. This change set does **not** bump either,
and nothing here uploads or submits. Confirm on App Store Connect whether build 9
was already uploaded before choosing values: if it was, a resubmit needs a new
`bundle.iOS.bundleVersion` (10). Never infer the uploaded state from local
metadata.

## Ship it

Same flow as before (not part of this change set):

```sh
./scripts/upload-appstore-ios.sh
```

or run `ios-appstore.yml`. The workflow flips `default = ["kofi", "p2p"]` to
`["appstore", "p2p"]` and strips the Ko-fi UI; P2P stays on in the store binary.
