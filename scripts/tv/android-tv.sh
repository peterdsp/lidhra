#!/usr/bin/env bash
# Build, install and run the Android TV / Google TV / Fire TV flavour of Lidhra.
#
# The TV flavour is the same Tauri app with:
#   - the `appstore` cargo feature and NO default features (no `kofi` updater,
#     no `p2p` engine: a TV streams from the debrid cloud, it does not download),
#   - app/src-tauri/tauri.tv.conf.json merged on top (main window opens
#     `index.html?tv=1`, which switches the web UI to its D-pad TV mode),
#   - a leanback-ready AndroidManifest (see scripts/tv/android/patch-manifest.py).
#
# Usage: scripts/tv/android-tv.sh <command> [options]
#   init                 `tauri android init` if gen/android is missing, then patch it
#   patch                re-apply the TV manifest/banner/icon patch only
#   build [--debug]      build the arm64 APK (default: release profile, signed
#                        with a local key, see "Signing" below); prints the APK path
#   emulator             create the `lidhra_tv` AVD if missing and start it
#                        (HEADLESS=1 for no window)
#   install              adb install -r the last built APK
#   run                  install + launch
#   screenshot [file]    save a PNG of the device screen
#   stop                 stop the $TV_AVD emulator (other emulators are left alone)
#   apk                  print the path of the last built APK
#
# Signing (release profile):
#   Set LIDHRA_ANDROID_KEYSTORE, LIDHRA_ANDROID_KEY_ALIAS and
#   LIDHRA_ANDROID_KEYSTORE_PASS (and optionally LIDHRA_ANDROID_KEY_PASS) to sign
#   with a real upload/release key. Without them the APK is signed with the
#   standard Android debug keystore (~/.android/debug.keystore), which is only
#   good for local testing and sideloading. No key material lives in the repo.
#
# Environment overrides: TAURI (CLI path), NDK_HOME, ANDROID_ENV (env script),
# TV_AVD (default lidhra_tv), TV_IMAGE (system image), TV_DEVICE (AVD profile),
# EMU_MEMORY / EMU_CORES / EMU_GPU, ADB_SERIAL (target device).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APP="$ROOT/app"
TAURI_DIR="$APP/src-tauri"
GEN="$TAURI_DIR/gen/android"
TV_CONF="$TAURI_DIR/tauri.tv.conf.json"
PKG="dev.peterdsp.lidhra"
OUT_DIR="$ROOT/app/src-tauri/target/android-tv"
LAST_APK_FILE="$OUT_DIR/last-apk"

TV_AVD="${TV_AVD:-lidhra_tv}"
TV_IMAGE="${TV_IMAGE:-system-images;android-34;android-tv;arm64-v8a}"
TV_DEVICE="${TV_DEVICE:-tv_1080p}"

info() { printf '\033[1;34m[android-tv]\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m[android-tv]\033[0m %s\n' "$*" >&2; exit 1; }

# ---- environment ------------------------------------------------------------
ANDROID_ENV="${ANDROID_ENV:-/Users/peterdsp/git/kujtesaPersonaleAI/bin/android/android-env.sh}"
if [[ -f "$ANDROID_ENV" ]]; then
  # shellcheck source=/dev/null
  source "$ANDROID_ENV"
else
  : "${ANDROID_HOME:=${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}"
  export ANDROID_HOME ANDROID_SDK_ROOT="$ANDROID_HOME"
  export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
fi
if [[ -z "${NDK_HOME:-}" ]]; then
  NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1 || true)"
fi
[[ -n "${NDK_HOME:-}" && -d "$NDK_HOME" ]] || die "no NDK found under $ANDROID_HOME/ndk (set NDK_HOME)"
export NDK_HOME ANDROID_NDK_HOME="$NDK_HOME"

TAURI="${TAURI:-$(command -v tauri || true)}"
[[ -n "$TAURI" ]] || TAURI="$HOME/.local/bin/tauri"
BUILD_TOOLS="$(ls -d "$ANDROID_HOME"/build-tools/* 2>/dev/null | sort -V | tail -1 || true)"

# Serial of the running emulator whose AVD is $TV_AVD (other emulators, e.g. a
# phone AVD, are left alone).
tv_serial() {
  local d
  for d in $(adb devices | awk '/^emulator-[0-9]+[[:space:]]+device$/{print $1}'); do
    if [[ "$(adb -s "$d" emu avd name 2>/dev/null | head -1 | tr -d '\r')" == "$TV_AVD" ]]; then
      printf '%s\n' "$d"; return 0
    fi
  done
  return 0
}

# adb against ADB_SERIAL, else the TV emulator, else the only attached device.
adb_() {
  local s="${ADB_SERIAL:-$(tv_serial)}"
  if [[ -n "$s" ]]; then adb -s "$s" "$@"; else adb "$@"; fi
}

# Absolute path of the npm CLI's tauri.js (the symlink target), used to repair
# the Gradle BuildTask (see patch-manifest.py --tauri-js).
tauri_js() {
  local p="$TAURI"
  while [[ -L "$p" ]]; do
    local t; t="$(readlink "$p")"
    [[ "$t" == /* ]] && p="$t" || p="$(cd "$(dirname "$p")" && cd "$(dirname "$t")" && pwd)/$(basename "$t")"
  done
  printf '%s\n' "$p"
}

# ---- commands ---------------------------------------------------------------
cmd_patch() {
  local js; js="$(tauri_js)"
  if [[ "$js" == *.js ]]; then
    python3 "$ROOT/scripts/tv/android/patch-manifest.py" --tauri-js "$js" --node "$(command -v node)"
  else
    python3 "$ROOT/scripts/tv/android/patch-manifest.py"
  fi
}

cmd_init() {
  [[ -x "$TAURI" ]] || die "Tauri CLI not found at $TAURI (npm i -g @tauri-apps/cli@2)"
  if [[ ! -f "$GEN/app/src/main/AndroidManifest.xml" ]]; then
    info "gen/android missing: running tauri android init"
    ( cd "$APP" && "$TAURI" android init --ci --skip-targets-install --config "$TV_CONF" )
  else
    info "gen/android exists, skipping init"
  fi
  [[ -f "$TAURI_DIR/icons/android-tv/tv_banner.png" ]] || python3 "$ROOT/scripts/tv/android/make-banner.py"
  cmd_patch
}

sign_apk() {
  local unsigned="$1" signed="$2"
  [[ -n "$BUILD_TOOLS" ]] || die "no Android build-tools found (need zipalign + apksigner)"
  local aligned="$OUT_DIR/aligned.apk"
  "$BUILD_TOOLS/zipalign" -f -p 4 "$unsigned" "$aligned"
  if [[ -n "${LIDHRA_ANDROID_KEYSTORE:-}" ]]; then
    [[ -n "${LIDHRA_ANDROID_KEY_ALIAS:-}" && -n "${LIDHRA_ANDROID_KEYSTORE_PASS:-}" ]] \
      || die "LIDHRA_ANDROID_KEYSTORE set but LIDHRA_ANDROID_KEY_ALIAS / LIDHRA_ANDROID_KEYSTORE_PASS missing"
    local keypass_var=LIDHRA_ANDROID_KEYSTORE_PASS
    [[ -n "${LIDHRA_ANDROID_KEY_PASS:-}" ]] && keypass_var=LIDHRA_ANDROID_KEY_PASS
    info "signing with release keystore $LIDHRA_ANDROID_KEYSTORE (alias $LIDHRA_ANDROID_KEY_ALIAS)"
    # Passwords are read from the environment by apksigner, never put on argv.
    "$BUILD_TOOLS/apksigner" sign --ks "$LIDHRA_ANDROID_KEYSTORE" --ks-key-alias "$LIDHRA_ANDROID_KEY_ALIAS" \
      --ks-pass env:LIDHRA_ANDROID_KEYSTORE_PASS --key-pass "env:$keypass_var" \
      --out "$signed" "$aligned"
  else
    local ks="$HOME/.android/debug.keystore"
    if [[ ! -f "$ks" ]]; then
      info "creating the standard Android debug keystore at $ks"
      mkdir -p "$HOME/.android"
      keytool -genkeypair -keystore "$ks" -storepass android -keypass android -alias androiddebugkey \
        -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Android Debug,O=Android,C=US" >/dev/null
    fi
    info "no release keystore configured: signing with the debug keystore (local testing only)"
    "$BUILD_TOOLS/apksigner" sign --ks "$ks" --ks-key-alias androiddebugkey \
      --ks-pass pass:android --key-pass pass:android --out "$signed" "$aligned"
  fi
  rm -f "$aligned" "$signed.idsig"
  "$BUILD_TOOLS/apksigner" verify "$signed" >/dev/null
}

cmd_build() {
  local debug=0
  [[ "${1:-}" == "--debug" ]] && debug=1
  [[ -f "$GEN/app/src/main/AndroidManifest.xml" ]] || cmd_init
  cmd_patch
  mkdir -p "$OUT_DIR"

  # --features appstore plus `-- --no-default-features` drops the `kofi`
  # (updater) and `p2p` defaults. The trailing args reach cargo through the
  # CLI's android-studio-script step.
  local args=(android build --apk --target aarch64 --config "$TV_CONF" --features appstore --ci)
  (( debug )) && args+=(--debug)
  info "tauri ${args[*]} -- --no-default-features"
  ( cd "$APP" && "$TAURI" "${args[@]}" -- --no-default-features )

  local apk
  if (( debug )); then
    apk="$(ls -t "$GEN"/app/build/outputs/apk/*/debug/*.apk 2>/dev/null | head -1 || true)"
    [[ -n "$apk" ]] || die "debug APK not found under $GEN/app/build/outputs/apk"
    cp "$apk" "$OUT_DIR/lidhra-tv-arm64-debug.apk"
    apk="$OUT_DIR/lidhra-tv-arm64-debug.apk"
  else
    local unsigned
    unsigned="$(ls -t "$GEN"/app/build/outputs/apk/*/release/*.apk 2>/dev/null | head -1 || true)"
    [[ -n "$unsigned" ]] || die "release APK not found under $GEN/app/build/outputs/apk"
    apk="$OUT_DIR/lidhra-tv-arm64.apk"
    sign_apk "$unsigned" "$apk"
  fi
  printf '%s\n' "$apk" >"$LAST_APK_FILE"
  info "APK: $apk ($(du -h "$apk" | cut -f1))"
}

last_apk() {
  [[ -f "$LAST_APK_FILE" ]] || die "no APK built yet: run '$0 build' first"
  local apk; apk="$(cat "$LAST_APK_FILE")"
  [[ -f "$apk" ]] || die "last APK $apk is gone: rebuild"
  printf '%s\n' "$apk"
}

cmd_emulator() {
  command -v emulator >/dev/null || die "emulator not on PATH ($ANDROID_HOME/emulator)"
  if ! emulator -list-avds | grep -qx "$TV_AVD"; then
    [[ -d "$ANDROID_HOME/${TV_IMAGE//;//}" ]] || die "system image $TV_IMAGE not installed (sdkmanager \"$TV_IMAGE\")"
    info "creating AVD $TV_AVD from $TV_IMAGE ($TV_DEVICE)"
    echo no | avdmanager create avd -n "$TV_AVD" -k "$TV_IMAGE" -d "$TV_DEVICE" --force >/dev/null
  fi
  local before; before="$(adb devices | awk '/^emulator-[0-9]+/{print $1}' | sort)"
  if [[ -n "$(tv_serial)" ]]; then
    info "$TV_AVD is already running ($(tv_serial)), reusing it"
  else
    local gpu mem
    if [[ "${HEADLESS:-0}" == "1" ]]; then gpu="${EMU_GPU:-swiftshader_indirect}"; mem=2048
    else gpu="${EMU_GPU:-auto}"; mem=2048; fi
    local eargs=(-avd "$TV_AVD" -gpu "$gpu" -memory "${EMU_MEMORY:-$mem}" -cores "${EMU_CORES:-2}"
      -no-boot-anim -no-snapshot-save -netdelay none -netspeed full)
    [[ "${HEADLESS:-0}" == "1" ]] && eargs+=(-no-window -no-audio)
    local log="${TMPDIR:-/tmp}/lidhra-tv-emulator.log"
    info "starting $TV_AVD (log: $log)"
    nohup emulator "${eargs[@]}" >"$log" 2>&1 &
  fi
  # Wait for the new emulator's serial to appear, then for it to finish booting.
  local serial="" t=0
  until [[ -n "$serial" ]]; do
    serial="$(tv_serial)"
    if [[ -z "$serial" ]]; then
      serial="$(comm -13 <(printf '%s\n' "$before") <(adb devices | awk '/^emulator-[0-9]+/{print $1}' | sort) | head -1)"
    fi
    [[ -n "$serial" ]] || { sleep 3; t=$((t + 3)); (( t < 180 )) || die "emulator did not appear in 180s"; }
  done
  adb -s "$serial" wait-for-device
  until [[ "$(adb -s "$serial" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]]; do
    sleep 3; t=$((t + 3))
    (( t < 400 )) || die "emulator did not finish booting"
  done
  info "emulator ready: $serial (commands target it automatically; ADB_SERIAL overrides)"
}

cmd_install() {
  local apk; apk="$(last_apk)"
  info "installing $apk"
  adb_ install -r "$apk"
}

cmd_run() {
  cmd_install
  info "launching $PKG"
  adb_ shell am start -n "$PKG/.MainActivity" >/dev/null
}

cmd_screenshot() {
  local out="${1:-$OUT_DIR/screen-$(date +%Y%m%d-%H%M%S).png}"
  mkdir -p "$(dirname "$out")"
  adb_ exec-out screencap -p >"$out"
  info "screenshot: $out"
}

cmd_stop() {
  local s; s="$(tv_serial)"
  if [[ -n "$s" ]]; then
    adb -s "$s" emu kill >/dev/null 2>&1 || true
    info "stopped $TV_AVD ($s)"
  else
    info "$TV_AVD is not running"
  fi
}

case "${1:-}" in
  init) cmd_init ;;
  patch) cmd_patch ;;
  build) shift; cmd_build "$@" ;;
  emulator) cmd_emulator ;;
  install) cmd_install ;;
  run) cmd_run ;;
  screenshot) shift; cmd_screenshot "$@" ;;
  stop) cmd_stop ;;
  apk) last_apk ;;
  ""|-h|--help|help) sed -n '2,33p' "$0" | sed 's/^# \{0,1\}//' ;;
  *) die "unknown command '$1' (see $0 --help)" ;;
esac
