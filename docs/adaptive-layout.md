# Adaptive layout & iPhone Duo (1.3.0)

Lidhra 1.3.0 makes the shared web UI adapt to changing display regions, and adds
the native geometry bridge iPhone Duo needs, without rewriting the app in
SwiftUI and without a second transfer engine. This note is the architecture and
the hand-off for the one part that cannot be finished on today's toolchain.

## What ships in 1.3.0

- A shared, content-driven adaptive layout in `ui/index.html`: `compact`,
  `regular`, `split`, and `stack` modes chosen from available space and any
  active display division, never from a model name, screen size, or orientation
  alone.
- One reusable selected-file detail surface that renders either in the modal
  bottom sheet (single-pane layouts) or inline beside / below the transfer list
  (two-pane layouts), with no duplicated IDs, handlers, or accessibility nodes.
- A versioned native → web geometry bridge (`NativePlugin.swift` →
  `window.__lidhraGeometry`) carrying a session id, scene bounds, the webview
  frame, all four safe-area insets and layout margins, size-class traits, a
  structured keyboard state (docked / floating / overlap), and active reserved
  regions. The web side accepts both the v1 and the richer v2 payload.
- A session-aware acceptance handshake: a new attachment (reload or webview
  process recovery) is a new session id that resets revision ordering, and the
  web UI pulls the first snapshot on page-ready (`geometry_sync`) so no initial
  emission is lost to a startup race.
- Progressive foldable-browser support: `window.viewport.segments` (or the older
  `visualViewport.segments`) is normalised into the same region contract, with an
  ordinary responsive fallback and no fabricated hinge.
- Share / Open In popovers anchored to the initiating control, mapped back to
  native points through the measured transform (`toNativeRect`) instead of a raw
  1:1 cast, so the anchor is correct under zoom or an inset host.
- Reserved-region insets mirrored to body-level overlays (modal sheets and the
  browser player) so a presentation stays clear of a fold or camera occlusion.
- Threshold hysteresis so a width hovering on a mode boundary does not oscillate,
  while a fold or occlusion still reacts immediately.
- The `100vh` / `100dvh` ordering fix, plus a visual-viewport recompute so the
  converted geometry follows pinch-zoom, scroll, and the keyboard.

## What does NOT ship as "supported" yet

The iPhone Duo native adaptive APIs (`UIArrangementViewController` and the
`UIView.reservedRegions` geometry) exist only in the **iOS 27.1 SDK (Xcode
27.1)**. The default App Store build still targets the iOS 26 / 27.0 SDK, where
those symbols are absent, so the Duo code is isolated behind the `LIDHRA_DUO`
compilation condition and a default build reports no regions and presents the
player full screen. The gated code is implemented and type-checked against the
real 27.1 SDK, but a Duo build is not shipped and the acceptance matrix has not
passed on an official Duo runtime, so 1.3.0 is **not** an iPhone-Duo-supported
release; its layout is Duo-*ready* and the native path is Duo-*implemented but
unverified on device*.

## Architecture

```
             native points                         CSS pixels
  ┌─────────────────────────────┐        ┌──────────────────────────────┐
  │ NativePlugin.swift           │  JSON  │ ui/index.html                │
  │  GeometryBridge  ────────────┼───────▶│  window.__lidhraGeometry     │
  │   scene / webview / safe /   │  (rev) │   -> LidhraAdaptive          │
  │   keyboard / regions[]       │        │       toCssRect / analyze /  │
  │  DuoRegions.active()  ← seam │        │       chooseLayout           │
  └─────────────────────────────┘        │   -> data-layout + CSS vars  │
                                          └──────────────────────────────┘
```

Layout state lives entirely in the web layer and is kept strictly separate from
engine and session state: a geometry event only re-lays-out the shell and can
never add, restart, cancel, or duplicate a transfer. The Rust engine bridge
(`lidhra_native_event`) is untouched.

### The webview stays full-scene

The webview must span the whole scene; the two work areas are drawn *inside* it
using the reported region geometry. Putting the single webview inside one
arrangement pane would not make its internal controls hinge-aware. The native
`ArrangementView` / `UIArrangementViewController` work (below) is reserved for
the native AVPlayer surface, not for hosting the webview.

## Geometry payload (contract v2, v1 still accepted)

`window.__lidhraGeometry(payload)` accepts the v2 object below; the shipped v1
shape (`{ v, rev, scene, webview, safeArea, keyboard:Number, regions }`) is still
accepted and normalised onto the same canonical form, so an older native binary
keeps working.

```jsonc
{
  "version": 2,                 // 2 or the legacy 1; other versions are dropped
  "revision": 42,               // strictly increasing within a session
  "sessionId": "…",             // per-attachment; a new id resets revision ordering
  "sceneId": "…",               // scene owner (scene session persistent id)
  "source": "native",           // native | browser-segments | responsive
  "coordinateSpace": "scene",
  "units": "points",
  "capabilities": { "regions": false }, // a real region provider vs an unimplemented one
  "webviewBounds": { "x": 0, "y": 0, "w": 1000, "h": 760 }, // webview frame in scene points
  "visibleBounds": { "x": 0, "y": 0, "w": 1000, "h": 760 },
  "safeArea":     { "top": 47, "right": 0, "bottom": 34, "left": 0 },
  "layoutMargins":{ "top": 0, "right": 16, "bottom": 0, "left": 16 },
  "traits": { "horizontalSizeClass": "regular", "verticalSizeClass": "compact" },
  "keyboard": { "docked": false, "floating": false, "overlap": 0 },
  "regions": [                  // reserved regions in scene points
    { "id": "r1", "kind": "division", "active": true, "rect": { "x": 489, "y": 0, "w": 22, "h": 760 } }
  ]
}
```

Acceptance and conversion, enforced by the web layer (`LidhraAdaptive`):

- **`normalizeSnapshot`** collapses v1 and v2 onto one canonical shape and
  rejects any malformed, negative-size, or unknown-version payload.
- **`acceptSnapshot`** is session-aware: a new `sessionId` returns `reset` (take
  it and restart ordering); within a session only a strictly newer `revision` is
  accepted, so stale, replayed, and wrong-session events drop and an old high
  revision can never contaminate a new attachment.
- **Points → CSS px uses a measured transform** (`deriveTransform`,
  `scale = innerWidth / webviewBounds.w`), never an assumed 1:1, and is recomputed
  from the retained raw snapshot on every visual-viewport change.
- **Safe-area insets are not applied here.** The CSS already uses
  `env(safe-area-inset-*)`; the payload's insets are for completeness / debugging.
- **A docked keyboard becomes a single bottom inset; a floating one does not**,
  so the keyboard is never subtracted twice.
- **Regions that no longer overlap the viewport are dropped**, so a fold that
  goes away clears its inset; a zero-thickness division is preserved.
- **A provider reporting no active regions** (`capabilities.regions:true`,
  empty `regions`) is distinct from an unimplemented provider (`false`): the
  former is a valid empty result, the latter falls back to responsive layout.

## Layout algorithm (`LidhraAdaptive.chooseLayout`)

Thresholds are content-driven and documented in the code
(`LIST_MIN 340`, `DETAIL_MIN 360`, `GAP 18`, `REGULAR_MIN 768`, `SIDEBAR 280`,
`STACK_MIN_H 220`, all CSS px):

| Situation | Mode | Behaviour |
| --- | --- | --- |
| Narrow window / outer display, no fold | `compact` | one pane, floating tab bar, detail in the modal sheet |
| ≥ 768 but no room for two work areas | `regular` | sidebar + single content pane, detail in the sheet |
| ≥ 768 with room for list + detail, or a central vertical fold | `split` | list and inline detail side by side |
| Central horizontal fold with tall halves (tabletop) | `stack` | list above, inline detail below, each scrollable |
| Central fold present | `split`/`stack` | the two work areas meet on the fold; the gutter matches the fold width |
| Fold squeezes the panes | — | the sidebar is dropped before the work areas are squeezed; if still too tight the smaller side is occluded and the layout falls back to a complete single pane |
| Constrained pane / large text | `compact`/`regular` | full single-pane flow; the detail stays reachable through a tap |

Continuity: the selected item and list position survive every mode change. When
a two-pane layout collapses to one pane the selection is preserved and no modal
is forced open; growing back restores the same inline detail. A progress poll
never re-fetches or resets an open detail pane.

## The iPhone Duo integration (implemented, gated behind `LIDHRA_DUO`)

Two native pieces are gated behind `LIDHRA_DUO` (a Swift active compilation
condition, **off** in a default build). They are now implemented against the real
iOS 27.1 SDK API, not a placeholder. The gate is still required because those
symbols are absent from the iOS 26 / 27.0 SDK, so a default build compiles them
out and stays buildable on the older toolchain; a Duo build sets the flag from
the environment through the plugin's `Package.swift`.

The API names were taken from the installed SDK, not from a doc: UIKit
`UIView.reservedRegions(kind:options:)` returning `UIView.ReservedRegion`
(`id`, `kind` = `.division` / `.occlusion`, `frame`, `margins`, `isActive`), and
`UIArrangementViewController` with `setViewController(_:for:)`
(`.primary` / `.secondary`) and `UISplitArrangement().axes(_:)`.

### 1. Reserved-region emitter, `DuoRegions.active(in:)`

Under `LIDHRA_DUO` it queries `view.reservedRegions(kind:options:)` for both
`.division` and `.occlusion` with `.includeInactive`, converts each `frame` from
the view's space into the window (scene) space that GeometryBridge reports the
webview frame in, and returns `{"id","kind","active","x","y","w","h"}` per
region (the bridge wraps the rect into the payload's `rect`). Inactive regions
travel with `active:false`; the web layer keeps the distinction but drops them
from the layout so an inactive division never opens a permanent gap.
`DuoRegions.supported` reports whether a real provider is compiled in, so the web
side can tell "no active regions" apart from "no provider".

### 2. Native player arrangement, `UIArrangementViewController`

Under `LIDHRA_DUO`, when a division is active `play(_:)` hosts the same
`AVPlayerViewController` as the `.primary` pane of a `UISplitArrangement` (axis
vertical) with a small `DuoPlaybackInfoController` (file title, close) as
`.secondary`; otherwise it presents full screen as before. Split, not overlay:
the two areas are simultaneously useful with no foreground/background
relationship, and primary/secondary are semantic roles the system places for the
pose. The `AVPlayer` / `AVPlayerViewController` instance is reused, never rebuilt,
so media identity and position are preserved.

Verification of the gated code against the iOS 27.1 SDK is recorded in
`docs/verification/iphone-duo/` (SDK type-check of the API usage at deployment
target 15.0, and full-file parse in both configs). Runtime pose validation on the
iPhone Duo simulator / device is tracked there too.

### Enabling the Duo build (Xcode 27.1)

1. Install Xcode 27.1 and confirm the iPhone Duo SDK and simulator (DeviceHub).
2. Build with `LIDHRA_DUO=1` in the environment, e.g.
   `LIDHRA_DUO=1 cargo tauri ios build …`. The `#if LIDHRA_DUO` guard lives on
   the **plugin** static target (`NativePlugin.swift`), not the generated app
   target, so a flag set only on the app (as an earlier draft of this note said)
   would never compile the Duo path in. The switch is therefore driven from the
   environment in the checked-in `app/src-tauri/plugins/native/ios/Package.swift`
   (`swiftSettings: [.define("LIDHRA_DUO")]` when the variable is set), which
   survives `cargo tauri ios init` regeneration because `Package.swift` is not
   generated. No `gen/apple` edit is required. Confirm from the build log that the
   plugin target compiled with the define and that the `#warning` fired.
3. Implement `DuoRegions.active(in:)` and the player arrangement against the
   confirmed API; keep the iOS 15 deployment target and both non-Duo builds
   compiling.
4. Run the acceptance matrix (`docs/verification/iphone-duo/`) on the official
   Duo runtime before calling 1.3.0 Duo-supported.

## Verification matrix

Environment: macOS (Darwin 27.0.0), Xcode 27.0 build 27A266a (iOS SDK 27.0),
Node 24 for the unit tests. iPhone Duo needs Xcode 27.1 (Apple lists it as
coming later this month; not installed here).

| Check | Result |
| --- | --- |
| `node --test test/adaptive.test.mjs` (conversion + inverse, transform derivation, v1/v2 normalisation, session reset/ordering, keyboard inset, hysteresis, segment folds, layout selection, thresholds, fold handling) | **pass** — 23/23 |
| App crate `cargo check --lib` (default and `appstore,p2p` feature sets) | **pass** |
| `crates` workspace `cargo build` / `cargo test` / `cargo clippy --all-targets -- -D warnings` (CI gate; unchanged by this work) | **pass** |
| `NativePlugin.swift` and `Package.swift` `swiftc -parse` against the iOS SDK (syntax only; Tauri/UIKit types not resolved) | **pass** |
| Browser (in-app): compact / regular / split by width; a synthetic v2 central vertical fold → `split` with the gutter aligned to the fold and the list basis on it; a stale revision is ignored; `LidhraAdaptive` exposes the full v2 API | **pass** (390 / 1200 px, synthetic geometry, clearly a simulation) |
| Swift `NativePlugin.swift` compiles / links; native geometry, handshake, keyboard classification on device | **not run** — needs a full iOS build (`cargo tauri ios build`); reviewed for SDK correctness |
| iPhone Duo poses (open-flat, book, tabletop), reserved-region-driven layout, native player arrangement | **blocked** — needs Xcode 27.1 + Duo runtime (not installed) |

Synthetic browser geometry exercises the layout algorithm and is labelled a
simulation; it is not evidence of on-device Duo support. The full acceptance
matrix and evidence index live under `docs/verification/iphone-duo/`.

## Files

- `ui/index.html` — `LidhraAdaptive` (marked `ADAPTIVE:START`/`END`), the
  geometry wiring (v2 acceptance, session state, visual-viewport recompute,
  browser segments), the dual-host detail surface with presentation intent and
  scroll continuity, layout CSS, overlay inset mirroring, the viewport fix.
- `app/src-tauri/plugins/native/ios/Sources/NativePlugin.swift` — `GeometryBridge`
  (v2 payload, session token, traits, structured keyboard, `requestSnapshot`),
  `DuoRegions` seam (`supported` + gated query), anchor-aware share / Open In.
- `app/src-tauri/plugins/native/ios/Package.swift` — env-driven `LIDHRA_DUO`
  define so the flag reaches the plugin compilation target durably.
- `app/src-tauri/plugins/native/src/lib.rs`, `app/src-tauri/src/lib.rs` — anchor
  and the `geometry_sync` handshake command plumbed through the Rust bridge.
- `test/adaptive.test.mjs` — unit tests that load the shipped `ADAPTIVE` block.
