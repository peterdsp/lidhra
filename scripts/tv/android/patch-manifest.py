#!/usr/bin/env python3
"""Apply the Android TV changes to the generated Tauri Android project.

`app/src-tauri/gen/` is gitignored and recreated by `tauri android init`, so
every TV-specific change lives here and is re-applied after each init. The
script is idempotent: running it twice leaves the project unchanged.

What it does:
  1. AndroidManifest.xml
     - <uses-feature android.software.leanback required=false>
     - <uses-feature android.hardware.touchscreen required=false>
       (both required=false so the same APK installs on phones, Google TV,
       Android TV and Fire TV)
     - LEANBACK_LAUNCHER next to LAUNCHER on the launcher activity
     - android:banner="@drawable/tv_banner" on <application>
  2. Copies the 320x180 xhdpi banner from app/src-tauri/icons/android-tv/ to
     res/drawable-xhdpi/tv_banner.png.
  3. Copies the brand launcher icons from app/src-tauri/icons/android/ over the
     Tauri placeholder mipmaps (what `tauri icon` would do).
  4. Optional (--tauri-js PATH): points the Gradle BuildTask at the npm Tauri
     CLI script. `tauri android init` run as `node tauri.js` records the command
     as `node tauri ...`, which Gradle then runs from app/src-tauri and fails
     (there is no file named `tauri` there). Pass the absolute path of the CLI's
     tauri.js to make the in-Gradle Rust build step work.

Usage: scripts/tv/android/patch-manifest.py [--tauri-js /abs/path/tauri.js] [--node /abs/path/node]
"""
import argparse
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
TAURI = ROOT / "app" / "src-tauri"
GEN = TAURI / "gen" / "android"
MAIN = GEN / "app" / "src" / "main"
MANIFEST = MAIN / "AndroidManifest.xml"
RES = MAIN / "res"
BANNER_SRC = TAURI / "icons" / "android-tv" / "tv_banner.png"
ICONS_SRC = TAURI / "icons" / "android"

LEANBACK = '<uses-feature android:name="android.software.leanback" android:required="false" />'
TOUCH = '<uses-feature android:name="android.hardware.touchscreen" android:required="false" />'
LB_CAT = '<category android:name="android.intent.category.LEANBACK_LAUNCHER" />'


def fail(msg: str) -> None:
    sys.exit(f"patch-manifest: {msg}")


def ensure_uses_feature(xml: str, name: str, line: str) -> str:
    pat = re.compile(r'<uses-feature\s+android:name="%s"[^>]*/>' % re.escape(name))
    if pat.search(xml):
        # Force required="false" on an existing declaration.
        return pat.sub(line, xml)
    # Insert right before <application so it sits with the other manifest-level tags.
    m = re.search(r"\n([ \t]*)<application\b", xml)
    if not m:
        fail("no <application> element in AndroidManifest.xml")
    indent = m.group(1)
    return xml[: m.start()] + f"\n{indent}{line}\n" + xml[m.start():]


def ensure_leanback_launcher(xml: str) -> str:
    # The launcher activity's intent-filter is the one holding category LAUNCHER.
    filt = re.compile(r"<intent-filter>(?:(?!</intent-filter>).)*?android\.intent\.category\.LAUNCHER\".*?</intent-filter>", re.S)
    m = filt.search(xml)
    if not m:
        fail("no intent-filter with category LAUNCHER found")
    block = m.group(0)
    if "android.intent.category.LEANBACK_LAUNCHER" in block:
        return xml
    lm = re.search(r'\n(\s*)<category android:name="android\.intent\.category\.LAUNCHER"\s*/>', block)
    new_block = block[: lm.end()] + f"\n{lm.group(1)}{LB_CAT}" + block[lm.end():]
    return xml[: m.start()] + new_block + xml[m.end():]


def ensure_banner(xml: str) -> str:
    m = re.search(r"<application\b([^>]*)>", xml, re.S)
    if not m:
        fail("no <application> element in AndroidManifest.xml")
    attrs = m.group(1)
    if 'android:banner="' in attrs:
        attrs = re.sub(r'android:banner="[^"]*"', 'android:banner="@drawable/tv_banner"', attrs)
    else:
        im = re.search(r"\n([ \t]*)android:icon=", attrs)
        indent = im.group(1) if im else "        "
        attrs = f"\n{indent}android:banner=\"@drawable/tv_banner\"" + attrs
    return xml[: m.start()] + f"<application{attrs}>" + xml[m.end():]


def copy_if_changed(src: Path, dst: Path) -> bool:
    if dst.exists() and dst.read_bytes() == src.read_bytes():
        return False
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src, dst)
    return True


def patch_build_task(tauri_js: str, node: str) -> bool:
    tasks = list((GEN / "buildSrc").rglob("BuildTask.kt"))
    if not tasks:
        fail("BuildTask.kt not found under gen/android/buildSrc")
    task = tasks[0]
    src = task.read_text()
    new = re.sub(r'val executable = """[^"]*""";', f'val executable = """{node}""";', src)
    new = re.sub(
        r'val args = listOf\([^)]*"android", "android-studio-script"\);',
        f'val args = listOf("""{tauri_js}""", "android", "android-studio-script");',
        new,
    )
    if new != src:
        task.write_text(new)
        return True
    return False


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tauri-js", help="absolute path to the npm Tauri CLI tauri.js")
    ap.add_argument("--node", default="node", help="node executable for the Gradle BuildTask")
    opts = ap.parse_args()

    if not MANIFEST.exists():
        fail(f"{MANIFEST} not found (run `tauri android init` from app/ first)")
    if not BANNER_SRC.exists():
        fail(f"{BANNER_SRC} not found (run scripts/tv/android/make-banner.py)")

    changes = []
    xml = MANIFEST.read_text()
    new = ensure_uses_feature(xml, "android.software.leanback", LEANBACK)
    new = ensure_uses_feature(new, "android.hardware.touchscreen", TOUCH)
    new = ensure_leanback_launcher(new)
    new = ensure_banner(new)
    if new != xml:
        MANIFEST.write_text(new)
        changes.append("AndroidManifest.xml")

    if copy_if_changed(BANNER_SRC, RES / "drawable-xhdpi" / "tv_banner.png"):
        changes.append("res/drawable-xhdpi/tv_banner.png")

    if ICONS_SRC.is_dir():
        for src in sorted(ICONS_SRC.rglob("*")):
            if src.is_file():
                rel = src.relative_to(ICONS_SRC)
                if copy_if_changed(src, RES / rel):
                    changes.append(f"res/{rel}")

    if opts.tauri_js:
        if patch_build_task(opts.tauri_js, opts.node):
            changes.append("buildSrc BuildTask.kt")

    # Verify the end state so a template change upstream fails loudly.
    final = MANIFEST.read_text()
    for needle in (
        'android:name="android.software.leanback" android:required="false"',
        'android:name="android.hardware.touchscreen" android:required="false"',
        "android.intent.category.LEANBACK_LAUNCHER",
        "android.intent.category.LAUNCHER",
        'android:banner="@drawable/tv_banner"',
    ):
        if needle not in final:
            fail(f"verification failed, missing: {needle}")

    print("patch-manifest: " + ("updated " + ", ".join(changes) if changes else "already up to date"))


if __name__ == "__main__":
    main()
