#!/usr/bin/env bash
# Package Lidhra for the TVs that run the shared web UI (plus the wasm bridge):
#
#   scripts/tv/build.sh web        build/tv/web/            self-contained bundle (also the hosted TV page)
#   scripts/tv/build.sh tizen      build/tv/Lidhra.wgt      Samsung Tizen TV (signed if the Tizen CLI + a profile exist)
#   scripts/tv/build.sh webos      build/tv/*.ipk           LG webOS TV (ares-package, or npx @webos-tools/cli)
#   scripts/tv/build.sh harmonyos  tv/harmonyos/            HarmonyOS project with the bundle in rawfile/ (build in DevEco)
#   scripts/tv/build.sh all
#
# Native TV apps live elsewhere: Apple TV in apps/tvos (scripts/tv/build-apple-tv-core.sh),
# Android / Google / Fire TV via Tauri (scripts/tv/android-tv.sh).
#
# Env: TIZEN_PROFILE=<security profile> to sign the .wgt; SKIP_WEB=1 to reuse build/tv/web.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/build/tv"
WEB="$OUT/web"
VERSION="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["version"])' "$ROOT/app/src-tauri/tauri.conf.json")"

log() { printf '==> %s\n' "$*"; }

web() {
  if [[ "${SKIP_WEB:-0}" == 1 && -f "$WEB/index.html" ]]; then log "Reusing $WEB"; return; fi
  "$ROOT/scripts/tv/build-web.sh"
  SKIP_WEB=1
}

# icon <out.png> <width> <height>: the app icon centred on the TV background colour.
icon() {
  python3 - "$ROOT/app/src-tauri/icons/icon.png" "$1" "$2" "$3" <<'PY'
import sys
from PIL import Image
src, out, w, h = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
bg = Image.new("RGBA", (w, h), (7, 17, 14, 255))
ic = Image.open(src).convert("RGBA")
side = int(min(w, h) * 0.78)
ic = ic.resize((side, side), Image.LANCZOS)
bg.alpha_composite(ic, ((w - side) // 2, (h - side) // 2))
bg.save(out)
PY
}

stage() { # stage <dir>: a fresh copy of the web bundle
  rm -rf "$1"; mkdir -p "$1"; cp -R "$WEB/." "$1/"
}

tizen() {
  web
  local src="$OUT/tizen/src"
  stage "$src"
  sed "s/@VERSION@/$VERSION/" "$ROOT/tv/tizen/config.xml" > "$src/config.xml"
  icon "$src/icon.png" 512 423   # Samsung TV app icon size
  rm -f "$OUT/Lidhra.wgt"
  if command -v tizen >/dev/null 2>&1 && [[ -n "${TIZEN_PROFILE:-}" ]]; then
    log "Signing with Tizen security profile '$TIZEN_PROFILE'"
    ( cd "$src" && tizen package -t wgt -s "$TIZEN_PROFILE" -o "$OUT" -- "$src" )
    mv "$OUT"/*.wgt "$OUT/Lidhra.wgt" 2>/dev/null || true
  else
    ( cd "$src" && zip -qr -X "$OUT/Lidhra-unsigned.wgt" . )
    log "Wrote build/tv/Lidhra-unsigned.wgt (UNSIGNED)."
    echo "    A Samsung TV only installs signed widgets. Install Tizen Studio (with the TV extension),"
    echo "    create a Samsung certificate profile in Certificate Manager, then:"
    echo "      TIZEN_PROFILE=<profile> scripts/tv/build.sh tizen"
    echo "    and install on a TV in Developer Mode: sdb connect <tv-ip> && tizen install -n Lidhra.wgt -t <device>"
    return
  fi
  log "Wrote build/tv/Lidhra.wgt"
}

webos() {
  web
  local src="$OUT/webos/src"
  stage "$src"
  sed "s/@VERSION@/$VERSION/" "$ROOT/tv/webos/appinfo.json" > "$src/appinfo.json"
  icon "$src/icon.png" 80 80
  icon "$src/largeIcon.png" 130 130
  local ares=()
  if command -v ares-package >/dev/null 2>&1; then ares=(ares-package)
  elif command -v npx >/dev/null 2>&1; then ares=(npx --yes -p @webos-tools/cli ares-package)
  fi
  if [[ ${#ares[@]} -eq 0 ]]; then
    log "No ares-package (npm i -g @webos-tools/cli). App folder ready at build/tv/webos/src"
    return
  fi
  rm -f "$OUT"/*.ipk
  "${ares[@]}" --no-minify "$src" -o "$OUT"
  log "Wrote $(ls "$OUT"/*.ipk)"
  echo "    Install on a TV in Developer Mode: ares-setup-device, then ares-install -d <tv> $(basename "$(ls "$OUT"/*.ipk)")"
}

harmonyos() {
  web
  local raw="$ROOT/tv/harmonyos/entry/src/main/resources/rawfile"
  find "$raw" -mindepth 1 ! -name .gitkeep -exec rm -rf {} +
  cp -R "$WEB/." "$raw/"
  if command -v hvigorw >/dev/null 2>&1; then
    ( cd "$ROOT/tv/harmonyos" && hvigorw assembleHap --mode module -p product=default -p buildMode=release --no-daemon )
    log "HAP under tv/harmonyos/entry/build/"
  else
    log "Bundle copied into tv/harmonyos/entry/src/main/resources/rawfile."
    echo "    Open tv/harmonyos in DevEco Studio, set signing (File > Project Structure > Signing Configs), and build a HAP."
  fi
}

case "${1:-}" in
  web) web ;;
  tizen) tizen ;;
  webos) webos ;;
  harmonyos) harmonyos ;;
  all) web; tizen; webos; harmonyos ;;
  *) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
