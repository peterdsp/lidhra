// swift-tools-version:5.3
import PackageDescription

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
      path: "Sources")
  ]
)
