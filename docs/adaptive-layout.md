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
  `window.__lidhraGeometry`) carrying scene bounds, the webview frame, all four
  safe-area insets, the keyboard overlap, and active reserved regions.
- Share / Open In popovers anchored to the initiating control instead of a
  fixed bottom-centre point.
- The `100vh` / `100dvh` ordering fix so the viewport tracks the keyboard.

## What does NOT ship as "supported" yet

The iPhone Duo native adaptive APIs (`ArrangementView` /
`UIArrangementViewController` and the reserved-region geometry) require **Xcode
27.1**, which Apple lists as *coming later this month* on
<https://developer.apple.com/iphone-duo/>. This project builds against the
**iOS 26 SDK (Xcode 26)**, where those symbols do not exist. The reserved-region
emitter is therefore isolated behind the `LIDHRA_DUO` compilation condition and
returns no regions on every currently shippable build. Until it is wired to the
confirmed API and the acceptance matrix passes on the official Duo runtime,
1.3.0 is **not** an iPhone-Duo-supported release; it is a release whose layout is
Duo-*ready*.

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

## Geometry payload (contract v1)

`window.__lidhraGeometry(payload)` accepts one JSON object:

```jsonc
{
  "v": 1,                       // contract version; other versions are dropped
  "rev": 42,                    // strictly increasing; stale/replayed are dropped
  "coordinateSpace": "scene",
  "scene":   { "w": 1000, "h": 760 },              // points
  "webview": { "x": 0, "y": 0, "w": 1000, "h": 760 }, // webview frame in scene points
  "safeArea":{ "top": 47, "right": 0, "bottom": 34, "left": 0 }, // points
  "keyboard": 0,                // points of the webview covered by the keyboard
  "regions": [                  // active reserved regions in scene points
    { "x": 489, "y": 0, "w": 22, "h": 760, "kind": "hinge" }
  ]
}
```

Conversion rules enforced by the web layer:

- **Points → CSS px uses a measured ratio**, `scale = innerWidth / webview.w`,
  never an assumed 1:1. A region is mapped with
  `(x - webview.x) * scale`, so a webview inset within the scene is handled.
- **Safe-area insets are not applied here.** The CSS already uses
  `env(safe-area-inset-*)`; applying the payload's insets too would pad twice.
  They are in the payload for completeness / debugging only.
- **Regions that no longer overlap the viewport are dropped**, so a fold that
  goes away clears its inset rather than leaving a stale gap.
- **A stale, replayed, or wrong-version payload is dropped** (`acceptRevision`).

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

## The isolated iPhone Duo integration

Two native pieces are gated behind `LIDHRA_DUO` (a Swift active compilation
condition, **off** in every current build). An `if #available` check is not
enough on its own: it cannot make a symbol that is absent from the iOS 26 SDK
compile, so the whole region-query body is compiled out until the flag is set in
an Xcode 27.1 build.

### 1. Reserved-region emitter — `DuoRegions.active(in:)`

Today it returns `[]` and the web UI falls back to an ordinary responsive
layout. Under `LIDHRA_DUO` there is a marked INTEGRATION POINT and a `#warning`:
query the scene's active division / reserved regions from the confirmed Duo
geometry API, map each rectangle into the scene coordinate space, and return
`{"x","y","w","h","kind"}` per region. Do not fabricate the symbol; wire the
confirmed one.

### 2. Native player arrangement (`ArrangementView` / `UIArrangementViewController`)

The full-screen `AVPlayerViewController` is unchanged for current builds. The
Duo enhancement — a primary playback area with a secondary information/actions
area for the selected file, in a `UIArrangementViewController` (split for two
visible areas; overlay only for a real foreground/background relationship),
reusing the single `AVPlayer` session across presentation changes — is likewise
Xcode-27.1-only and must be proven in a minimal working slice before it is
wired into `play(_:)`.

### Enabling the Duo build (Xcode 27.1)

1. Install Xcode 27.1 and confirm the iPhone Duo SDK and simulator (DeviceHub).
2. Add `LIDHRA_DUO` to `SWIFT_ACTIVE_COMPILATION_CONDITIONS` for the iOS target.
   `gen/apple` is regenerated by `cargo tauri ios init`, so set it at the source
   in `app/src-tauri/gen/apple/project.yml` (target `settings.base`), e.g.
   `SWIFT_ACTIVE_COMPILATION_CONDITIONS: $(inherited) LIDHRA_DUO`, so the flag
   survives regeneration.
3. Implement `DuoRegions.active(in:)` and the player arrangement against the
   confirmed API; keep the iOS 15 deployment target and both non-Duo builds
   compiling.
4. Run the acceptance matrix (below) on the official Duo runtime before calling
   1.3.0 Duo-supported.

## Verification matrix

Environment: macOS 26 (Darwin 25.6.0), Xcode 26.6 (iOS SDK 26.5), Rust 1.98.1,
Node for the unit tests. iPhone Duo needs Xcode 27.1 (not yet released).

| Check | Result |
| --- | --- |
| `node --test test/adaptive.test.mjs` (region conversion, layout selection, thresholds, fold handling, revision ordering) | **pass** — 15/15 |
| App crate `cargo check --lib` (default and `appstore,p2p` feature sets) | **pass** |
| `crates` workspace `cargo test` and `cargo clippy --all-targets -- -D warnings` (CI gate; unchanged by this work) | **pass** |
| `NativePlugin.swift` `swiftc -parse` against the iOS 26 simulator SDK (syntax only; Tauri/UIKit types not resolved) | **pass** |
| Browser: compact / regular / split by width; tap-to-select fills the inline detail; selection + list position preserved shrinking to compact and growing back; compact tap opens the modal sheet | **pass** (in-app browser, 390 / 834 / 1000 / 1280 px) |
| Browser: synthetic central vertical fold → `split`, sidebar collapses to the tab bar, gutter aligns to the fold | **pass** (synthetic geometry, clearly a simulation) |
| Swift `NativePlugin.swift` compiles / runs | **unavailable in this environment** — needs a full iOS build; reviewed for iOS 26 SDK correctness |
| iPhone Duo poses (open-flat, book, tabletop), reserved-region-driven layout, native player arrangement | **unavailable** — needs Xcode 27.1 + Duo runtime |

Synthetic browser geometry exercises the layout algorithm and is labelled a
simulation; it is not evidence of on-device Duo support.

## Files

- `ui/index.html` — `LidhraAdaptive` (marked `ADAPTIVE:START`/`END`), the
  geometry wiring, the dual-host detail surface, layout CSS, the viewport fix.
- `app/src-tauri/plugins/native/ios/Sources/NativePlugin.swift` — `GeometryBridge`,
  `DuoRegions` seam, anchor-aware share / Open In.
- `app/src-tauri/plugins/native/src/lib.rs`, `app/src-tauri/src/lib.rs` — anchor
  plumbed through the Rust bridge.
- `test/adaptive.test.mjs` — unit tests that load the shipped `ADAPTIVE` block.
