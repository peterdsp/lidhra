// swift-tools-version:5.3
import PackageDescription
import Foundation

// The reserved-region / arrangement code in NativePlugin.swift is guarded by
// `#if LIDHRA_DUO`. That flag lives on *this* plugin target, not the generated
// app target, so setting it only on the app (as an earlier note suggested)
// would never compile the Duo path in. Driving it from the environment keeps
// the switch in one durable, checked-in place that survives `tauri ios init`
// regeneration: build the Duo variant with `LIDHRA_DUO=1` in the environment
// (e.g. `LIDHRA_DUO=1 cargo tauri ios build ...` on an Xcode 27.1 toolchain).
// Absent the variable the placeholder empty-region provider stays the default,
// so no other configuration changes.
let duoEnabled = ProcessInfo.processInfo.environment["LIDHRA_DUO"] != nil
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
