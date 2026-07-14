#!/usr/bin/env bash
# Build the iOS App Store IPA and upload it to App Store Connect, automatically,
# using the App Store Connect API key (no Transporter clicking).
#
# Prereqs (one-time):
#   - The Lidhra app RECORD must exist in App Store Connect (bundle
#     dev.peterdsp.lidhra, platform iOS). Create it once at
#     https://appstoreconnect.apple.com/apps  (+  ->  New App).
#   - Apple signing certs in your Keychain (Apple Distribution, team YTS4KJBX3P).
#   - App Store Connect API key at ~/.appstoreconnect/private_keys/AuthKey_<KEYID>.p8
#
# Usage:  ./scripts/upload-appstore-ios.sh
set -euo pipefail
cd "$(dirname "$0")/.."

ASC_KEY_ID="${ASC_KEY_ID:-B3BD3SK79A}"
ASC_ISSUER_ID="${ASC_ISSUER_ID:-94b49788-ea57-4033-af26-dc5d362f185e}"
TEAM="${APPLE_DEVELOPMENT_TEAM:-YTS4KJBX3P}"

# Xcode's Swift must win over any swiftly/rustup-less shim; homebrew rust can't
# cross-compile to iOS, so use rustup's cargo. This PATH excludes ~/.swiftly/bin.
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 APPLE_DEVELOPMENT_TEAM="$TEAM"
unset TOOLCHAINS || true

echo "==> Building signed App Store IPA (appstore feature: no self-updater)"
pushd app/src-tauri >/dev/null
cp Cargo.toml /tmp/Lidhra.Cargo.toml.bak
sed -i.bak 's/^default = \["kofi"\]/default = ["appstore"]/' Cargo.toml && rm -f Cargo.toml.bak
popd >/dev/null

cleanup() { cp /tmp/Lidhra.Cargo.toml.bak app/src-tauri/Cargo.toml 2>/dev/null || true; }
trap cleanup EXIT

# App Store icons must be fully opaque (no alpha). Flatten the generated iOS
# AppIcon set onto its own dark tile colour (Apple masks the corners anyway).
ICONSET="app/src-tauri/gen/apple/Assets.xcassets/AppIcon.appiconset"
if [ -d "$ICONSET" ]; then
  /usr/bin/python3 - "$ICONSET" <<'PY'
import sys, glob
from PIL import Image
for f in glob.glob(sys.argv[1] + "/*.png"):
    im = Image.open(f).convert("RGBA"); w, h = im.size; px = im.load()
    fill = (18, 24, 36)
    for x in range(w):
        r, g, b, a = px[x, h // 2]
        if a > 250: fill = (r, g, b); break
    bg = Image.new("RGB", im.size, fill); bg.paste(im, mask=im.split()[3]); bg.save(f)
print("flattened iOS AppIcon (removed alpha)")
PY
fi

( cd app && cargo tauri ios build --export-method app-store-connect )
cleanup; trap - EXIT

IPA=$(ls -t app/src-tauri/gen/apple/build/*/*.ipa 2>/dev/null | head -1)
[ -n "$IPA" ] || { echo "no IPA produced"; exit 1; }
cp "$IPA" "$HOME/Downloads/Lidhra.ipa"
echo "==> Built: $IPA (copied to ~/Downloads/Lidhra.ipa)"

echo "==> Validating with App Store Connect"
xcrun altool --validate-app -f "$IPA" -t ios --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
echo "==> Uploading to App Store Connect"
xcrun altool --upload-app -f "$IPA" -t ios --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
echo "Done. The build will appear in App Store Connect > Lidhra > TestFlight in a few minutes."
