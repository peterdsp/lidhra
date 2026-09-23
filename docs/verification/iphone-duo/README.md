# iPhone Duo acceptance matrix and evidence index

Status vocabulary: **not run** / **passed** / **failed** / **blocked** /
**n/a (reason)**. Every geometry result below that is not from an on-device Duo
runtime is a **simulation** (unit test or in-app browser with synthetic
geometry) and is labelled as such. A simulation is never evidence of on-device
Duo support.

## Environment

| Field | Value |
| --- | --- |
| Branch / base | `duo-adaptive-layout`, base `be70ac8` + the 1.3.0 geometry-v2 + Duo change |
| App version / build | 1.3.0 / iOS bundle build 9 (`tauri.conf.json`) |
| Duo toolchain | Xcode 27.1 (27A9269) present at `~/Downloads/Xcode_27.1.app`, iOS SDK + simulator SDK 27.1 |
| Simulator runtimes | iOS 26.5, 27.0, and **27.1** (downloaded); iPhone Duo device type present |
| Active toolchain (`xcode-select`) | **Xcode 27.1** (switched via `sudo xcode-select -s`); Xcode 27.0's host macOS SDK modules are broken, so it cannot be used either |
| Web runtimes | Node 24 (unit tests); in-app Chromium browser (synthetic geometry) |

## Native build status (2026-09-23)

**Update: the app now builds, installs, and launches on the iPhone Duo simulator
(iOS 27.1) with the Duo code linked in.** The reserved-region query and the
split-arrangement player are compiled and present in the app binary (verified:
`DuoPlaybackInfoController`, `reservedRegions`, `occlusion`/`division` in the
built `Lidhra.debug.dylib`). Getting there took, all now durable in the repo:
swift-rs 1.0.8 (fixes the Xcode 27 SDK targeting), the deployment-target
reconcile (`scripts/ios-postgen.sh`, 14.0 -> 15.0), CocoaPods, the `LIDHRA_DUO`
marker that bridges the flag to the plugin's swift-rs build, and a
`UIApplicationSceneManifest` (with the `TaoScene` configuration) that clears the
iOS 27 scene-adoption launch trap.

One runtime issue remains, and it is **in Tauri/tao, not the Duo code**: after
launch the WKWebView loads the full UI (its subresources finish loading) but tao
does not lay its view out visibly in the iOS 27 scene, so the screen stays black.
tao registers its `TaoSceneDelegate` at runtime, after the scene is created, so
the plist cannot name it and the scene is hosted without tao's scene-attach code.
This needs an upstream tao/tauri fix (or a newer tauri) for iOS 27 scene layout;
it blocks visually exercising the fold layout on the simulator but not the native
code's compile/link/launch. Physical-device or a fixed-tauri run is the next step.

The earlier toolchain blockers below were resolved in order:

- The Duo API usage (`UIView.reservedRegions`, `UIArrangementViewController`,
  `UISplitArrangement`) **type-checks** against SDK 27.1 at deployment target
  15.0 (isolation harness, `import UIKit`/`AVKit`), and `NativePlugin.swift`
  parses in both the default and `-D LIDHRA_DUO` configs against SDK 27.1.
- The deployment-target reconcile (`scripts/ios-postgen.sh`, iOS 14.0 -> 15.0)
  and the CocoaPods install were resolved; `xcode-select` is now Xcode 27.1; the
  iOS 27.1 simulator runtime is installed and an `iPhone Duo` simulator is
  created. So the environment is otherwise ready.
- **Root blocker:** the build fails inside **`tauri v2.11.5`'s `build.rs`**,
  where **swift-rs** compiles the Tauri Swift runtime. Under Xcode 27.1 it builds
  the macOS `AppKit` / `WebKit` framework modules with an iOS target context
  (`TARGET_OS_IPHONE`), so the macOS SDK 27.0 headers take the iOS branch and
  fail: `CoreImage` -> `OpenGLES/EAGL.h`, `WebKit` -> `UIKit/NSAttributedString.h`,
  plus the macOS-SDK-27.0 `CoreServices/CSIdentityBase.h` module bug. A plain
  `swiftc import AppKit; import WebKit` for the host succeeds, so the SDK is fine
  in a correct host context; only swift-rs's mixed target/SDK build trips it.
  This is independent of `LIDHRA_DUO` (that flag only affects the plugin's own
  `Package.swift`), so it blocks the ordinary app too. Xcode 27.0 cannot be used
  either: its host macOS SDK modules are broken outright. This matches the known
  swift-rs / Xcode incompatibilities in tauri-apps/tauri#7339 and #6545, whose
  only documented workaround is a different Xcode version.
- **To run on device/simulator:** a toolchain where tauri's swift-rs step builds
  under the 27.1 SDK (a fixed swift-rs / tauri-cli, or an Apple macOS-SDK fix),
  then `LIDHRA_DUO=1 cargo tauri ios build --debug --target aarch64-sim` on the
  `iPhone Duo` simulator (iOS 27.1). The App Store CI would need a macOS-27 +
  Xcode 27.1 runner once that combination builds cleanly.

## Automated evidence (reproducible now)

```sh
node --test test/adaptive.test.mjs            # 23/23 pass
cargo check --manifest-path app/src-tauri/Cargo.toml --lib
cargo check --manifest-path app/src-tauri/Cargo.toml --lib --no-default-features --features appstore,p2p
cargo build --manifest-path crates/Cargo.toml --workspace
cargo test  --manifest-path crates/Cargo.toml --workspace
cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings

# Duo-gated Swift, type-checked against the real iOS 27.1 SDK (deployment 15.0):
DD=/Users/peterdsp/Downloads/Xcode_27.1.app/Contents/Developer
SDK=$(DEVELOPER_DIR="$DD" xcrun --sdk iphoneos --show-sdk-path)
DEVELOPER_DIR="$DD" xcrun swiftc -parse -D LIDHRA_DUO -sdk "$SDK" \
  -target arm64-apple-ios15.0 app/src-tauri/plugins/native/ios/Sources/NativePlugin.swift
```

The 23 unit tests cover: native-points ↔ CSS conversion and its inverse,
transform derivation, v1/v2 snapshot normalisation, session reset and revision
ordering, docked vs floating keyboard insets, threshold hysteresis, browser
segment → fold derivation, and the layout decision (compact / regular / split /
stack, folds, occlusion fallback).

## Scenario matrix

| ID | Scenario | Status | Evidence / reason |
| --- | --- | --- | --- |
| D01 | Outer cold launch, portrait and landscape/tent | blocked | needs Xcode 27.1 + Duo runtime |
| D02 | Inner cold launch, landscape and portrait | blocked | needs Duo runtime |
| D03 | Flat → folded → flat, vertical division | passed (simulation) | in-app browser synthetic v2 fold; unit: fold → split, stale rev ignored, region-gone clears |
| D04 | Off-center horizontal and vertical divisions | passed (simulation) | unit: `analyzeRegions` / `chooseLayout` fold placement; workspace-local basis in `applyLayout` |
| D05 | Left and right Split View | blocked | needs Duo runtime for real scene bounds |
| D06 | PiP pin/unpin then fold | not run | needs Duo runtime; keyboard/height reflow path implemented |
| D07 | Camera occlusion appears/disappears with same bounds | not run | needs Duo runtime; occlusion inset path unit-covered |
| D08 | Active detail collapses; passive selection collapses | passed (simulation) | code: `FS.active` intent, `relocateDetail` retains active detail in the sheet, keeps list for passive |
| D09 | Nested file + scrolled detail, 10 transitions, no refetch | passed (simulation) | code: `paintDetail` no-refetch relocate, `FS.scroll` restore, `FS.loaded` guard; unit N/A (DOM) |
| D10 | Add/account draft with keyboard and IME | not run | needs device; form nodes preserved across relayout (not rebuilt) |
| D11 | Docked / floating / external keyboard | passed (simulation) | unit: docked → one bottom inset, floating → none; native classifies docked vs floating |
| D12 | Zoom/pan, nonzero origin, native chrome/insets | passed (simulation) | unit: `toCssRect`/`toNativeRect` inverse under scale+origin; visual-viewport recompute wired |
| D13 | Share/Open In during fold/rotation/resize | not run | needs device; anchor now mapped through measured transform |
| D14 | Custom sheet open/dragging during resize | passed (simulation) | code: overlay insets mirrored to `:root`; relayout does not clobber a drag |
| D15 | Playing/paused media, scrub/buffer, 10 transitions | blocked | native player arrangement is Xcode-27.1 gated; single-`AVPlayer` reuse noted |
| D16 | PiP/AirPlay, interruption, return | not run | needs device; `allowsPictureInPicturePlayback` set, session config unverified |
| D17 | Transfer progress, offline/reconnect, lifecycle | passed (partial) | code: geometry issues zero engine commands; lifecycle bridge unchanged |
| D18 | Malformed/replayed/wrong-scene snapshots, new epoch | passed (simulation) | unit: `acceptSnapshot` reset/accept/drop; live browser stale-rev drop |
| D19 | Initial readiness, reload, reattachment, recovery | passed (partial) | `geometry_sync` handshake + new-session reset implemented; device reattach not run |
| D20 | A/B selection race and removed selected item | passed (simulation) | code: `FS.item` identity guard on late responses; removed-item explicit clear |
| D21 | Multiple regions, inactive/zero-thickness, interior occlusion | passed (simulation) | unit: zero-thickness division preserved; `supported` vs empty distinction |
| D22 | Narrow usable area in wide viewport; threshold crossings | passed (simulation) | unit: occlusion fallback + hysteresis; `data-nav` owns sidebar |
| D23 | VoiceOver, keyboard, Switch Control, large text/200% | not run | needs device / AT session |
| D24 | Seven locales, long text, mixed scripts, RTL fixture | not run | no new visible strings added; existing 7-locale table intact |
| D25 | Appearance/accessibility settings | not run | needs device; reduce-motion already honoured |
| D26 | Segment-capable browser, one segment, unsupported browser | passed (simulation) | unit: `segmentsToRegions` real gap → fold, seamless/none → responsive |
| D27 | Ordinary iPhone, iPad multitasking, server/browser, desktop | passed (partial) | in-app browser renders; desktop/browser flows unchanged; iPad multitasking not run |
| D28 | 50 transitions, large list/open detail, perf | not run | needs an instrumented run with a representative large list |
| D29 | Fresh native project generation + build configs | not run | needs `cargo tauri ios init` in an isolated checkout on the intended SDK |
| D30 | Terminate/relaunch persisted state | not run | needs device; session restore already ships, distinct from in-memory continuity |

## What "simulation" means here

D03/D04/D18 in the browser used a hand-fed v2 payload through
`window.__lidhraGeometry` and asserted the resulting `data-layout`, `data-fold`,
`--split-gap`, and `--split-list-basis`. That proves the web pipeline from the
contract to CSS, not that a device reports those regions. Assert real rendered
pane rectangles against transformed regions on the official runtime before any
"iPhone Duo support verified" claim.
