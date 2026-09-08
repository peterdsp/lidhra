# Vendored crates

Pure Rust copies of upstream crates that need a small fix before they build for
every Lidhra target. Each folder is the crates.io tarball plus the patch described
below, wired in through `[patch.crates-io]` in `crates/Cargo.toml` and
`app/src-tauri/Cargo.toml`. Drop the folder and the patch line once upstream
ships the fix.

## librqbit-dualstack-sockets 0.7.0

`src/bind_device.rs` gates the per-interface socket binding on
`target_os = "macos"`, so the iOS targets fell through to the Linux branch and
failed with `no method named bind_device found for &Socket`. The two cfg gates now
use `target_vendor = "apple"`, which covers macOS and iOS alike. Nothing else changed.
