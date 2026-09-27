# Lidhra on Android TV, Google TV and Fire TV

The TV flavour of Lidhra is a **library and player**, not a downloader: connect a
debrid provider, browse the cloud transfers, pick a file and play it. It is the
same Tauri app (`app/src-tauri`) and the same web UI (`ui/index.html`) as every
other build, with three differences:

| What | How |
|------|-----|
| Cargo features | `--no-default-features --features appstore`: no `kofi` (trial, license key, self-updater, which is desktop only) and no `p2p` (a TV streams, it does not store downloads). |
| Config overlay | `app/src-tauri/tauri.tv.conf.json` is merged with `--config`. The main window opens `index.html?tv=1`, which switches the web UI into its D-pad TV mode, full screen. |
| Manifest | `scripts/tv/android/patch-manifest.py` makes the generated Android project leanback ready (details below). |

One APK serves Google TV, Android TV, Fire TV and (because `LAUNCHER` is kept and
nothing TV-only is required) phones and tablets.

## Files

| Path | Purpose |
|------|---------|
| `scripts/tv/android-tv.sh` | Entry point: `init`, `patch`, `build`, `emulator`, `install`, `run`, `screenshot`, `stop`, `apk`. |
| `scripts/tv/android/patch-manifest.py` | Idempotent patch of `gen/android` after every `tauri android init`. |
| `scripts/tv/android/make-banner.py` | Renders the TV banner from `design/Brand` (Pillow). Output is committed. |
| `app/src-tauri/tauri.tv.conf.json` | TV config overlay. |
| `app/src-tauri/icons/android-tv/tv_banner.png` | 320x180 launcher banner (xhdpi). |
| `app/src-tauri/icons/android-tv/tv_banner_1280x720.png` | Google Play TV banner (store listing asset). |

`app/src-tauri/gen/` is gitignored (like `gen/apple` for iOS), so the Android
project is generated on demand and every TV change is applied by the patch
script, never by hand.

### What the patch does

- `<uses-feature android:name="android.software.leanback" android:required="false"/>`
  (the Tauri 2.11 template already declares it; the script enforces it).
- `<uses-feature android:name="android.hardware.touchscreen" android:required="false"/>`
  (Google Play and the Amazon Appstore hide apps that implicitly require touch).
- `android.intent.category.LEANBACK_LAUNCHER` on the launcher activity, next to
  `LAUNCHER` (Fire TV and phones use `LAUNCHER`, Android TV / Google TV use
  `LEANBACK_LAUNCHER`).
- `android:banner="@drawable/tv_banner"` on `<application>`, and copies the
  banner to `res/drawable-xhdpi/tv_banner.png`.
- Copies the brand launcher icons from `app/src-tauri/icons/android/` over the
  Tauri placeholder mipmaps (what `tauri icon` would do).
- Repairs the Gradle `BuildTask.kt` when the npm Tauri CLI is invoked directly
  (`~/.local/bin/tauri` is a symlink to `tauri.js`): `tauri android init`
  records the Rust build step as `node tauri android android-studio-script`,
  which Gradle runs from `app/src-tauri` where no `tauri` file exists. The
  script passes the absolute `node` and `tauri.js` paths instead. This only
  touches the generated (gitignored) project.

### Identifier

The Android `applicationId` / namespace is `dev.peterdsp.lidhra`, the same as
the other builds. It is a valid Java package name, so Android needs no change.
If the TV build ever has to coexist with a phone build on the same device or as
a separate Play listing, give the overlay its own `identifier` (for example
`dev.peterdsp.lidhra.tv`) and re-run `init` (the identifier is baked into the
generated project, including the Kotlin package path).

## Prerequisites

- npm Tauri CLI 2 (`~/.local/bin/tauri`, checked with 2.11.4). `cargo tauri` is not installed on this Mac.
- Android SDK at `~/Library/Android/sdk`: platform 36 (compileSdk / targetSdk of
  the Tauri template), build-tools (zipalign, apksigner), NDK (checked with
  30.0.16248370), emulator and `system-images;android-34;android-tv;arm64-v8a`.
- JDK 17. On this Mac `source /Users/peterdsp/git/kujtesaPersonaleAI/bin/android/android-env.sh`
  sets `JAVA_HOME`, `ANDROID_HOME` and `PATH`; the script sources it when it
  exists (override with `ANDROID_ENV=`) and otherwise falls back to
  `~/Library/Android/sdk`.
- Rust target `aarch64-linux-android` (`rustup target add aarch64-linux-android`).
- `NDK_HOME` defaults to the newest NDK under `$ANDROID_HOME/ndk`.

## Build, install, run

```sh
scripts/tv/android-tv.sh init        # tauri android init (if gen/android is missing) + patch
scripts/tv/android-tv.sh build       # arm64 release APK, signed (see Signing)
scripts/tv/android-tv.sh build --debug   # debug Rust profile, Gradle debug-signed
HEADLESS=1 scripts/tv/android-tv.sh emulator   # create + boot the lidhra_tv AVD
scripts/tv/android-tv.sh run         # adb install -r + launch
scripts/tv/android-tv.sh screenshot docs/tv/android-tv-screenshots/x.png
scripts/tv/android-tv.sh stop        # stops only the lidhra_tv emulator
```

The build runs, from `app/`:

```sh
tauri android build --apk --target aarch64 \
  --config src-tauri/tauri.tv.conf.json --features appstore --ci \
  -- --no-default-features
```

`--features` / `-f` is the CLI's own flag; everything after `--` is appended to
the `cargo build` command line. The resulting cargo invocation is
`cargo build --package lidhra-desktop --target aarch64-linux-android --features appstore tauri/custom-protocol --no-default-features --lib --release`.

Output: `app/src-tauri/target/android-tv/lidhra-tv-arm64.apk` (the unsigned
Gradle output lives in `gen/android/app/build/outputs/apk/universal/release/`).
`scripts/tv/android-tv.sh apk` prints the last built path.

The `emulator` command creates `lidhra_tv` (`tv_1080p` profile, override with
`TV_DEVICE`, `TV_IMAGE`, `TV_AVD`) and never reuses or stops another running
emulator: every adb command targets the `lidhra_tv` serial (or `ADB_SERIAL`).

## Signing

- **Default (local testing):** the release APK is zipaligned and signed with the
  standard Android debug keystore `~/.android/debug.keystore` (created with the
  well-known `android` / `androiddebugkey` values if missing). Good for the
  emulator and for sideloading, not for a store.
- **Release:** keep the keystore outside the repo and export

  ```sh
  export LIDHRA_ANDROID_KEYSTORE=/secure/path/lidhra-upload.jks
  export LIDHRA_ANDROID_KEY_ALIAS=upload
  export LIDHRA_ANDROID_KEYSTORE_PASS=...      # read by apksigner from env, never on argv
  export LIDHRA_ANDROID_KEY_PASS=...           # optional, defaults to the store password
  scripts/tv/android-tv.sh build
  ```

  Create the key once with `keytool -genkeypair -v -keystore lidhra-upload.jks -alias upload -keyalg RSA -keysize 2048 -validity 10000`.
  For Google Play use Play App Signing: this key is the **upload** key, Google
  holds the app signing key. In CI store the `.jks` as a base64 secret and
  decode it to a temp file. `gen/android/.gitignore` already ignores
  `key.properties` / `keystore.properties` if you prefer Gradle signing configs
  (Tauri docs: "Android code signing"), but that means editing the generated
  `build.gradle.kts`, so the apksigner route above is what the script uses.
- Google Play requires an **AAB** for new apps: `tauri android build --aab ...`
  with the same flags (the script builds APKs; add `--aab` when a store upload
  pipeline is set up). Fire TV sideloading and the Amazon Appstore take APKs.

## Fire TV notes

- Same APK. Fire OS 7/8 is Android 9/11 based (API 28/30); the APK's
  `minSdk` is 24, so every current Fire TV stick runs it. Fire TV 4K Max and
  newer are arm64; older sticks are 32-bit only, so add `--target armv7` (and
  install the `armv7-linux-androideabi` Rust target) to reach them.
- Sideload: enable Developer options, ADB debugging and "Install unknown apps",
  then `adb connect <fire-tv-ip>:5555` and
  `ADB_SERIAL=<fire-tv-ip>:5555 scripts/tv/android-tv.sh run`.
- Amazon Appstore submission: upload the release-signed APK; Amazon re-signs
  it. `android.hardware.touchscreen required=false` is mandatory for Fire TV
  targeting, and the `LAUNCHER` category is what Fire TV's launcher uses
  (it ignores `LEANBACK_LAUNCHER`). Amazon asks for its own 1280x720 banner
  and 512x512 icon in the listing.
- The Fire TV remote is a standard D-pad: `KEYCODE_DPAD_UP/DOWN/LEFT/RIGHT`,
  `KEYCODE_DPAD_CENTER` (select), `KEYCODE_BACK`, `KEYCODE_MENU`, plus media
  keys (`KEYCODE_MEDIA_PLAY_PAUSE`, `_REWIND`, `_FAST_FORWARD`). The WebView
  delivers the D-pad as `ArrowUp`/`ArrowDown`/`ArrowLeft`/`ArrowRight`/`Enter`
  key events, which is what the web TV mode listens to; Back arrives as a
  system back (the Tauri activity finishes unless the page handles history).

## Google Play (TV) checklist

| Requirement | Status |
|-------------|--------|
| `android.software.leanback` declared (`required="false"` to share the phone APK) | done (patch) |
| `LEANBACK_LAUNCHER` intent filter on the launcher activity | done (patch) |
| `android:banner` 320x180 xhdpi on `<application>` | done (`tv_banner.png`) |
| Touchscreen not required | done (patch) |
| No other hardware implicitly required (camera, GPS, telephony, portrait) | holds: the manifest only asks for `INTERNET` |
| 64-bit native code | done (`arm64-v8a`); add `x86_64` only if Play flags it for ChromeOS/emulators |
| Full D-pad navigation, visible focus, no touch-only UI | web UI TV mode (`?tv=1`), owned by the UI work |
| Landscape, content inside TV safe area | the TV mode layout, owned by the UI work |
| targetSdk within Play's current window | 36 from the Tauri template |
| AAB upload + Play App Signing | to do when a store pipeline is added |
| TV screenshots (1920x1080) and 1280x720 banner in the listing | banner in `icons/android-tv/tv_banner_1280x720.png` |
| Opt in to "Android TV" form factor in Play Console and pass TV review | to do at submission |

## Verified results

Verified on 2026-09-27 on this Mac (Apple silicon, macOS 27), Tauri CLI 2.11.4,
tauri 2.11.5, NDK 30.0.16248370, JDK 17.0.20, Gradle 8.14.3.

- `scripts/tv/android-tv.sh init` generated `gen/android` and patched it; a
  second `patch` run reported "already up to date" (idempotent).
- `scripts/tv/android-tv.sh build` succeeded with no Cargo.toml or Rust
  changes. Cargo ran with `--features appstore tauri/custom-protocol --no-default-features --lib --release`
  for `aarch64-linux-android`. NDK r30 needed no workaround.
- APK: `app/src-tauri/target/android-tv/lidhra-tv-arm64.apk`, 11 MB
  (11,069,186 bytes; `liblidhra_desktop_lib.so` is 8.0 MB uncompressed),
  `arm64-v8a` only, versionCode 1003000 / versionName 1.3.0, minSdk 24,
  targetSdk 36, signed with the debug keystore (`apksigner verify` passes).
- `aapt dump badging` shows `leanback-launchable-activity`,
  `launchable-activity`, `banner=...`, and both `android.software.leanback`
  and `android.hardware.touchscreen` as `uses-feature-not-required`.
- The packaged `assets/tauri.conf.json` has the main window at
  `index.html?tv=1`, full screen. No `librqbit` or updater symbols are in the
  native library (P2P and updater compiled out).
- Emulator `lidhra_tv` (`android-34;android-tv;arm64-v8a`, `tv_1080p`,
  headless, swiftshader): installed with `adb install -r`, launched, and the
  web UI loaded in TV mode: the "Connect a provider" grid with the copy
  "Nothing is downloaded to this TV", which comes from `?tv=1`. D-pad keys
  (`KEYCODE_DPAD_*`) move a visible focus ring across the provider cards
  (no ring is drawn before the first few presses; the initial focus is up to
  the web TV mode).
  A cold start through the `LEANBACK_LAUNCHER` intent took 2.7 s on a heavily
  loaded host. Stopped at the connect screen (no credentials entered).
- The Android TV launcher lists Lidhra in **Apps** with the generated banner.
- Seen once during testing: the emulator's `system_server` restarted while the
  host load average was above 60 (other builds running). Lidhra was not in the
  foreground at the time and the crash log only shows `DeadSystemException` in
  Google apps; after the restart Lidhra launched normally.

Screenshots (960x540, downscaled from 1920x1080):

- `android-tv-screenshots/01-launcher-apps-row.png`: launcher Apps row with the Lidhra banner.
- `android-tv-screenshots/02-connect-screen-tv-mode.png`: first launch, TV mode connect screen.
- `android-tv-screenshots/03-dpad-focus.png`: D-pad focus on a provider card.
