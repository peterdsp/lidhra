#!/usr/bin/env bash
# Build the self-contained web-TV bundle that Tizen, webOS, HarmonyOS and the
# hosted TV page all package: the shared UI + the lidhra-bridge wasm module.
#
#   scripts/tv/build-web.sh            -> build/tv/web/
#
# Needs: rustup target wasm32-unknown-unknown, wasm-bindgen CLI matching the
# crate version (cargo install wasm-bindgen-cli --version <x> --locked).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/build/tv/web"
TARGET_DIR="$ROOT/crates/target"
WASM="$TARGET_DIR/wasm32-unknown-unknown/release/lidhra_bridge.wasm"

need() { command -v "$1" >/dev/null 2>&1 || { echo "error: '$1' not found. $2" >&2; exit 1; }; }
need cargo "Install Rust from https://rustup.rs"
need wasm-bindgen "Run: cargo install wasm-bindgen-cli --version \$(grep -A1 '^name = \"wasm-bindgen\"$' crates/Cargo.lock | sed -n 's/version = \"\\(.*\\)\"/\\1/p') --locked"
need python3 "python3 is used to inline the wasm and inject the boot scripts"

echo "==> Building lidhra-bridge for wasm32 (size-optimised release)"
# Size matters on TVs: optimise for size, one codegen unit, LTO, abort on panic.
( cd "$ROOT/crates" && \
  CARGO_PROFILE_RELEASE_OPT_LEVEL=z CARGO_PROFILE_RELEASE_LTO=true \
  CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_PANIC=abort \
  cargo build -p lidhra-bridge --release --target wasm32-unknown-unknown )

echo "==> Generating JS bindings"
rm -rf "$OUT"; mkdir -p "$OUT/bridge"
# no-modules: a plain global `wasm_bindgen`, since older TV engines lack ES modules.
wasm-bindgen --target no-modules --no-typescript --out-dir "$OUT/bridge" "$WASM"

echo "==> Assembling bundle"
python3 - "$ROOT" "$OUT" <<'PY'
import base64, pathlib, shutil, sys
root, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
wasm = (out / "bridge" / "lidhra_bridge_bg.wasm").read_bytes()
(out / "bridge" / "lidhra_bridge_wasm.js").write_text(
    "/* lidhra-bridge wasm, base64 (see tv/web/tv-boot.js). Generated. */\n"
    "window.LIDHRA_BRIDGE_WASM=\"" + base64.b64encode(wasm).decode() + "\";\n")
(out / "bridge" / "lidhra_bridge_bg.wasm").unlink()
shutil.copy(root / "tv" / "web" / "tv-boot.js", out / "tv-boot.js")
html = (root / "ui" / "index.html").read_text(encoding="utf-8")
anchor = '<meta charset="utf-8">'
assert html.count(anchor) == 1, "ui/index.html: charset meta not found"
boot = ('\n<script src="bridge/lidhra_bridge.js"></script>'
        '\n<script src="bridge/lidhra_bridge_wasm.js"></script>'
        '\n<script src="tv-boot.js"></script>')
(out / "index.html").write_text(html.replace(anchor, anchor + boot), encoding="utf-8")
shutil.copy(root / "app" / "src-tauri" / "icons" / "icon.png", out / "icon.png")
print(f"   wasm {len(wasm)/1024:.0f} KiB -> base64 {len(wasm)*4/3/1024:.0f} KiB")
PY
du -sh "$OUT" | awk '{print "==> build/tv/web ready ("$1")"}'
