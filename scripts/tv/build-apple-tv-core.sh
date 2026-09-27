#!/usr/bin/env bash
# Build the Rust core (crates/lidhra-bridge) for Apple TV and package it as
# apps/tvos/LidhraCore.xcframework (device + simulator static libraries, plus
# the C header and module map so Swift can `import LidhraBridge`).
#
# Usage: scripts/tv/build-apple-tv-core.sh
#   PROFILE=debug          build the debug profile instead of release
#   CARGO_TARGET_DIR=...   use a different cargo target directory
#   TVOS_DEPLOYMENT_TARGET minimum tvOS version (default 17.0, matches the app)
#
# Idempotent: rebuilds incrementally and replaces the xcframework each run.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CRATES="$ROOT/crates"
BRIDGE="$CRATES/lidhra-bridge"
OUT="$ROOT/apps/tvos/LidhraCore.xcframework"
PROFILE="${PROFILE:-release}"
TARGET_DIR="${CARGO_TARGET_DIR:-$CRATES/target}"
LIB="liblidhra_bridge.a"

# Rust (target spec) and the C compiler used by build scripts such as `ring`
# both read this, so every object in the archive agrees on the minimum OS.
export TVOS_DEPLOYMENT_TARGET="${TVOS_DEPLOYMENT_TARGET:-17.0}"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null || die "cargo not found (install Rust via rustup)"
command -v xcodebuild >/dev/null || die "xcodebuild not found (install Xcode)"
xcrun --sdk appletvos --show-sdk-path >/dev/null 2>&1 || die "tvOS SDK missing (Xcode > Settings > Components)"
xcrun --sdk appletvsimulator --show-sdk-path >/dev/null 2>&1 || die "tvOS Simulator SDK missing"

cargo_flags=(--release)
[[ "$PROFILE" == "debug" ]] && cargo_flags=()

# target triple : Xcode SDK name
TARGETS=("aarch64-apple-tvos:appletvos" "aarch64-apple-tvos-sim:appletvsimulator")

if command -v rustup >/dev/null; then
  installed="$(rustup target list --installed)"
  for entry in "${TARGETS[@]}"; do
    triple="${entry%%:*}"
    grep -qx "$triple" <<<"$installed" || { say "Installing Rust target $triple"; rustup target add "$triple"; }
  done
fi

build_target() {
  local triple="$1" sdk="$2" sdkroot cc ar env_triple
  sdkroot="$(xcrun --sdk "$sdk" --show-sdk-path)"
  cc="$(xcrun --sdk "$sdk" --find clang)"
  ar="$(xcrun --sdk "$sdk" --find ar)"
  # cc-rs reads CC_<triple> / AR_<triple> with dashes turned into underscores.
  env_triple="${triple//-/_}"
  say "Building lidhra-bridge for $triple ($PROFILE, tvOS $TVOS_DEPLOYMENT_TARGET+)"
  (
    cd "$CRATES"
    env \
      SDKROOT="$sdkroot" \
      "CC_${env_triple}=$cc" \
      "AR_${env_triple}=$ar" \
      CARGO_TARGET_DIR="$TARGET_DIR" \
      cargo build -p lidhra-bridge "${cargo_flags[@]}" --target "$triple"
  ) || die "cargo build failed for $triple"
  [[ -f "$TARGET_DIR/$triple/$PROFILE/$LIB" ]] || die "missing $TARGET_DIR/$triple/$PROFILE/$LIB"
}

for entry in "${TARGETS[@]}"; do
  build_target "${entry%%:*}" "${entry##*:}"
done

# Headers live in a subdirectory named after the module, so the module map
# never collides with another library's module.modulemap in the build include dir.
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/lidhra-tv-core.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/Headers/LidhraBridge"
cp "$BRIDGE/include/lidhra_bridge.h" "$BRIDGE/include/module.modulemap" "$STAGE/Headers/LidhraBridge/"

say "Packaging $OUT"
rm -rf "$OUT"
xcodebuild -create-xcframework \
  -library "$TARGET_DIR/aarch64-apple-tvos/$PROFILE/$LIB" -headers "$STAGE/Headers" \
  -library "$TARGET_DIR/aarch64-apple-tvos-sim/$PROFILE/$LIB" -headers "$STAGE/Headers" \
  -output "$OUT" >/dev/null || die "xcodebuild -create-xcframework failed"

for slice in "$OUT"/*/; do
  [[ -f "$slice/$LIB" ]] || continue
  printf '    %s  %s\n' "$(basename "$slice")" "$(du -h "$slice/$LIB" | cut -f1)"
done
say "Done. Open apps/tvos/Lidhra.xcodeproj and build the Lidhra scheme."
