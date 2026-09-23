// swift-tools-version:5.3
import PackageDescription
import Foundation

// The reserved-region / arrangement code in NativePlugin.swift is guarded by
// `#if LIDHRA_DUO`. This plugin's Swift is compiled by swift-rs from the plugin
// crate's build.rs (not Xcode/SPM/CocoaPods), and the `LIDHRA_DUO` environment
// variable does not survive Tauri's `xcode-script -> cargo -> swift-rs` chain to
// reach this manifest. So the switch is driven by a marker file next to this
// manifest, which `scripts/ios-postgen.sh` writes when it is run with
// `LIDHRA_DUO` set (that script runs in the outer shell, where the variable is
// present) and removes otherwise. The environment variable is still honoured for
// a direct `swift build`. Absent both, the placeholder empty-region provider
// stays the default, so no other configuration changes. Build the Duo variant
// with `LIDHRA_DUO=1 cargo tauri ios build ...` on an Xcode 27.1 toolchain.
// swift-rs runs `swift build` with the working directory set to this `ios/`
// package dir, so a working-directory-relative check finds the marker reliably;
// SwiftPM may evaluate the manifest from a temp copy, so `#filePath` is only a
// fallback. The environment variable is honoured too for a direct `swift build`.
let duoFM = FileManager.default
let duoByCwd = duoFM.fileExists(atPath: duoFM.currentDirectoryPath + "/.lidhra_duo")
let duoByFile = duoFM.fileExists(atPath: URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent(".lidhra_duo").path)
let duoEnabled = ProcessInfo.processInfo.environment["LIDHRA_DUO"] != nil || duoByCwd || duoByFile
let duoSettings: [SwiftSetting] = duoEnabled ? [.define("LIDHRA_DUO")] : []

let package = Package(
  name: "tauri-plugin-lidhra-native",
  platforms: [
    .iOS(.v13)
  ],
  products: [
    .library(
      name: "tauri-plugin-lidhra-native",
      type: .static,
      targets: ["tauri-plugin-lidhra-native"])
  ],
  dependencies: [
    // Copied here by the `tauri-plugin` build script.
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-lidhra-native",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources",
      swiftSettings: duoSettings)
  ]
)
