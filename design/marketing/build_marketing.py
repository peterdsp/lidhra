#!/usr/bin/env python3
"""
Generate Lidhra marketing images for App Store Connect and Ko-fi.

Outputs SVGs plus rasterized PNGs (via rsvg-convert) into this folder:

  appstore/macos/*.png   2560x1600  (16:10, App Store macOS)
  appstore/ios/*.png     1290x2796  (6.7" iPhone)
  kofi/*.png             cover + gallery (16:10)

One reusable Lidhra window / phone-screen component is drawn once and
re-used across every frame so the product UI stays consistent.

Run:  python3 build_marketing.py
"""
import os
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))

# ---- palette (from the design tokens) ------------------------------------
TEAL, GREEN, LIME = "#15c3b6", "#2fd191", "#54e06a"
INK, INK2 = "#0a1512", "#0c1a16"
MUTED, FAINT = "#46584f", "#7a8a82"
MIST = "#f2f8f5"
CARD, BORDER, ROW = "#ffffff", "#d8e4dd", "#f6faf8"
DL, SEED, PAUSE = "#22bd7a", "#11a594", "#8a94a8"
SANS = "Helvetica Neue, Helvetica, Arial, sans-serif"
MONO = "Menlo, Consolas, monospace"

DEFS = f'''<defs>
<linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
 <stop offset="0" stop-color="{TEAL}"/><stop offset=".55" stop-color="{GREEN}"/><stop offset="1" stop-color="{LIME}"/></linearGradient>
<linearGradient id="gh" x1="0" y1="0" x2="1" y2="0">
 <stop offset="0" stop-color="{TEAL}"/><stop offset=".55" stop-color="{GREEN}"/><stop offset="1" stop-color="{LIME}"/></linearGradient>
<linearGradient id="ink" x1="0" y1="0" x2="1" y2="1">
 <stop offset="0" stop-color="#07201b"/><stop offset="1" stop-color="#0a2a24"/></linearGradient>
<radialGradient id="glow" cx="80%" cy="6%" r="70%">
 <stop offset="0" stop-color="{GREEN}" stop-opacity=".22"/><stop offset="1" stop-color="{GREEN}" stop-opacity="0"/></radialGradient>
<radialGradient id="glowd" cx="82%" cy="0%" r="80%">
 <stop offset="0" stop-color="{GREEN}" stop-opacity=".38"/><stop offset="1" stop-color="{GREEN}" stop-opacity="0"/></radialGradient>
<filter id="sh" x="-20%" y="-20%" width="140%" height="150%">
 <feDropShadow dx="0" dy="26" stdDeviation="34" flood-color="#0a2a24" flood-opacity="0.22"/></filter>
</defs>'''

# App icon (the cloud mark), embedded as a data URI so every SVG is self-contained.
import base64 as _b64
_ICON = _b64.b64encode(open(os.path.join(HERE, "app-icon-256.png"), "rb").read()).decode()
_ICON_DATA = "data:image/png;base64," + _ICON

def mark(x, y, px, stroke=None, w=None):
    # stroke/w kept for call-site compatibility; the raster icon carries its own color.
    return f'<image href="{_ICON_DATA}" x="{x}" y="{y}" width="{px}" height="{px}"/>'

def esc(t):
    return t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

def txt(x, y, s, size, weight=400, fill=INK2, font=SANS, anchor="start", ls=None):
    a = f' text-anchor="{anchor}"' if anchor != "start" else ""
    l = f' letter-spacing="{ls}"' if ls else ""
    return (f'<text x="{x}" y="{y}" font-family="{font}" font-size="{size}" '
            f'font-weight="{weight}" fill="{fill}"{a}{l}>{esc(s)}</text>')

# ---------------------------------------------------------------------------
# Reusable macOS app window, drawn inside a 1000 x 620 local box at (0,0).
# ---------------------------------------------------------------------------
def mac_window():
    p = ['<g filter="url(#sh)">']
    p.append(f'<rect x="0" y="0" width="1000" height="620" rx="18" fill="{CARD}" stroke="{BORDER}"/>')
    p.append('</g>')
    # title bar
    p.append(f'<rect x="0" y="0" width="1000" height="52" rx="18" fill="{ROW}"/>')
    p.append(f'<rect x="0" y="34" width="1000" height="18" fill="{CARD}"/>')
    p.append(f'<rect x="0" y="0" width="1000" height="620" rx="18" fill="none" stroke="{BORDER}"/>')
    for cx, c in ((30, "#ff5f57"), (54, "#febc2e"), (78, "#28c840")):
        p.append(f'<circle cx="{cx}" cy="26" r="8" fill="{c}"/>')
    p.append(mark(112, 12, 28))
    p.append(txt(150, 32, "Lidhra", 17, 700, INK2))
    p.append(txt(970, 32, "↓ 12.4  ·  ↑ 2.1 MB/s", 14, 500, FAINT, MONO, "end"))
    # sidebar
    p.append(f'<rect x="1" y="52" width="252" height="567" fill="#f9fcfa"/>')
    p.append(f'<line x1="253" y1="52" x2="253" y2="619" stroke="{BORDER}"/>')
    p.append(txt(30, 96, "LIBRARY", 13, 700, FAINT, MONO, ls="1.8"))
    nav = [("◆ Overview", "12", True), ("◆ Transfers", "3", False),
           ("◆ Seeding", "7", False), ("◆ Paused", "2", False)]
    y = 112
    for label, count, active in nav:
        if active:
            p.append(f'<rect x="18" y="{y}" width="220" height="34" rx="9" fill="#dcf1e7"/>')
        col = "#0b8f79" if active else MUTED
        wt = 600 if active else 400
        p.append(txt(34, y + 22, label, 16, wt, col))
        p.append(txt(226, y + 22, count, 13, 400, FAINT, MONO, "end"))
        y += 40
    p.append(txt(30, y + 30, "CATEGORIES", 13, 700, FAINT, MONO, ls="1.8"))
    y += 46
    for label, count in (("◆ Linux", "8"), ("◆ Media", "4"), ("◆ ISO images", "6")):
        p.append(txt(34, y + 22, label, 16, 400, MUTED))
        p.append(txt(226, y + 22, count, 13, 400, FAINT, MONO, "end"))
        y += 38
    # provider chip
    p.append(f'<rect x="18" y="548" width="220" height="52" rx="12" fill="{ROW}" stroke="{BORDER}"/>')
    p.append(f'<circle cx="44" cy="574" r="12" fill="url(#g)"/>')
    p.append(txt(66, 570, "Real-Debrid", 13.5, 600, INK2))
    p.append(txt(66, 588, "connected", 11.5, 400, "#0b8f79", MONO))
    # main toolbar
    p.append(f'<rect x="282" y="80" width="86" height="34" rx="9" fill="url(#gh)"/>')
    p.append(txt(325, 102, "+ Add", 14.5, 700, "#06251b", SANS, "middle"))
    p.append(f'<rect x="378" y="80" width="96" height="34" rx="9" fill="#eef4f0" stroke="{BORDER}"/>')
    p.append(txt(426, 102, "Pause all", 13, 400, MUTED, SANS, "middle"))
    p.append(txt(968, 96, "⌘F  Filter…", 13, 400, FAINT, MONO, "end"))
    p.append(txt(968, 118, "3 active · 7 seeding", 12, 400, FAINT, MONO, "end"))
    # transfer rows
    rows = [
        ("ubuntu-24.04.2-desktop-amd64.iso", "4.7 GB · 62% · 214 peers", 0.62, DL, "12.4 MB/s", "14m"),
        ("debian-13-netinst.iso", "631 MB · seeding · ratio 3.4", 1.0, SEED, "↑ 2.1", "∞"),
        ("blender-4.6-linux-x64.tar.xz", "312 MB · 88% · direct link", 0.88, DL, "9.7 MB/s", "22s"),
        ("archlinux-2026.07.01-x86_64.iso", "1.1 GB · paused · 88%", 0.88, PAUSE, "-", "-"),
    ]
    y = 150
    for name, meta, prog, col, spd, eta in rows:
        p.append(txt(282, y, name, 15.5, 600, INK2))
        p.append(txt(282, y + 20, meta, 12.5, 400, FAINT, MONO))
        p.append(f'<rect x="282" y="{y+30}" width="560" height="7" rx="3.5" fill="#c9d8d0"/>')
        p.append(f'<rect x="282" y="{y+30}" width="{560*prog:.0f}" height="7" rx="3.5" fill="{col}"/>')
        p.append(txt(968, y + 2, spd, 14, 600, col, MONO, "end"))
        p.append(txt(968, y + 22, eta, 12.5, 400, FAINT, MONO, "end"))
        p.append(f'<line x1="282" y1="{y+52}" x2="968" y2="{y+52}" stroke="{BORDER}"/>')
        y += 74
    return "".join(p)

# ---------------------------------------------------------------------------
# Reusable iOS screen, drawn inside a 390 x 844 local box at (0,0).
# ---------------------------------------------------------------------------
def ios_screen():
    p = [f'<rect x="0" y="0" width="390" height="844" rx="0" fill="{MIST}"/>']
    p.append(f'<rect x="0" y="0" width="390" height="844" fill="url(#glow)"/>')
    # status bar
    p.append(txt(24, 40, "9:41", 15, 700, INK2, MONO))
    p.append(txt(366, 40, "↓ 12.4  ↑ 2.1", 12.5, 500, FAINT, MONO, "end"))
    # nav
    p.append(mark(24, 66, 30))
    p.append(txt(62, 92, "Lidhra", 26, 800, INK2))
    p.append(f'<circle cx="356" cy="84" r="18" fill="url(#g)"/>')
    p.append(txt(356, 91, "+", 22, 700, "#06251b", SANS, "middle"))
    # segmented
    p.append(f'<rect x="24" y="112" width="342" height="40" rx="12" fill="#e7f1ec"/>')
    p.append(f'<rect x="27" y="115" width="112" height="34" rx="10" fill="{CARD}" stroke="{BORDER}"/>')
    p.append(txt(83, 137, "Active", 14, 600, INK2, SANS, "middle"))
    p.append(txt(197, 137, "Seeding", 14, 400, FAINT, SANS, "middle"))
    p.append(txt(311, 137, "Done", 14, 400, FAINT, SANS, "middle"))
    # cards
    cards = [
        ("ubuntu-24.04.iso", "4.7 GB · 62%", 0.62, DL, "12.4 MB/s", "14 min"),
        ("debian-13.iso", "seeding · ratio 3.4", 1.0, SEED, "↑ 2.1 MB/s", "∞"),
        ("blender-4.6.tar.xz", "88% · direct link", 0.88, DL, "9.7 MB/s", "22 sec"),
        ("fedora-40-ws.iso", "paused · 40%", 0.40, PAUSE, "paused", "-"),
    ]
    y = 176
    for name, meta, prog, col, spd, eta in cards:
        p.append(f'<rect x="24" y="{y}" width="342" height="104" rx="18" fill="{CARD}" stroke="{BORDER}"/>')
        p.append(f'<rect x="40" y="{y+20}" width="34" height="34" rx="9" fill="{col}"/>')
        p.append(txt(88, y + 34, name, 15, 600, INK2))
        p.append(txt(88, y + 54, meta, 12.5, 400, FAINT, MONO))
        p.append(f'<rect x="40" y="{y+72}" width="310" height="7" rx="3.5" fill="#c9d8d0"/>')
        p.append(f'<rect x="40" y="{y+72}" width="{310*prog:.0f}" height="7" rx="3.5" fill="{col}"/>')
        p.append(txt(350, y + 34, spd, 12.5, 600, col, MONO, "end"))
        p.append(txt(350, y + 54, eta, 11.5, 400, FAINT, MONO, "end"))
        y += 120
    # tab bar
    p.append(f'<rect x="0" y="768" width="390" height="76" fill="{CARD}"/>')
    p.append(f'<line x1="0" y1="768" x2="390" y2="768" stroke="{BORDER}"/>')
    tabs = [("Transfers", True), ("Seeding", False), ("Account", False)]
    tx = 65
    for label, active in tabs:
        col = "#0b8f79" if active else FAINT
        p.append(f'<circle cx="{tx}" cy="798" r="6" fill="{col}"/>')
        p.append(txt(tx, 826, label, 12, 600 if active else 400, col, SANS, "middle"))
        tx += 130
    # home indicator
    p.append(f'<rect x="150" y="832" width="90" height="5" rx="2.5" fill="#c9d8d0"/>')
    return "".join(p)

# ---------------------------------------------------------------------------
def svg_open(w, h):
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">{DEFS}'

def light_bg(w, h):
    return (f'<rect width="{w}" height="{h}" fill="{MIST}"/>'
            f'<rect width="{w}" height="{h}" fill="url(#glow)"/>')

def dark_bg(w, h):
    return (f'<rect width="{w}" height="{h}" fill="url(#ink)"/>'
            f'<rect width="{w}" height="{h}" fill="url(#glowd)"/>')

def render(name, svg):
    svg_path = os.path.join(HERE, name + ".svg")
    png_path = os.path.join(HERE, name + ".png")
    os.makedirs(os.path.dirname(svg_path), exist_ok=True)
    with open(svg_path, "w") as f:
        f.write(svg)
    subprocess.run(["rsvg-convert", "-o", png_path, svg_path], check=True)
    print("  " + name + ".png")

# ---- macOS App Store screenshots  (2560 x 1600) ---------------------------
def macos_frame(name, headline, sub, win_extra="", dark=False):
    W, H = 2560, 1600
    s = svg_open(W, H)
    s += dark_bg(W, H) if dark else light_bg(W, H)
    hc = "#ffffff" if dark else INK
    sc = "#bfe9d8" if dark else MUTED
    s += txt(W//2, 220, headline, 96, 800, hc, SANS, "middle")
    s += txt(W//2, 300, sub, 44, 400, sc, SANS, "middle")
    # window centered, scaled from 1000x620
    scale = 1.86
    ww, wh = 1000*scale, 620*scale
    wx, wy = (W-ww)/2, 400
    s += f'<g transform="translate({wx:.0f},{wy:.0f}) scale({scale})">{mac_window()}{win_extra}</g>'
    s += "</svg>"
    render(name, s)

# ---- iOS App Store screenshots (1290 x 2796) ------------------------------
def premium_bg(w, h):
    # Rich dark backdrop with flowing brand-coloured waves (klipa-style polish).
    s = f'<rect width="{w}" height="{h}" fill="url(#ink)"/>'
    s += f'<rect width="{w}" height="{h}" fill="url(#glowd)"/>'
    for yb, op, sw in [(0.16, 0.20, 4), (0.24, 0.15, 3), (0.33, 0.11, 3),
                       (0.62, 0.10, 3), (0.72, 0.13, 4), (0.82, 0.10, 3)]:
        y = h * yb
        d = (f"M -60 {y:.0f} C {w*0.30:.0f} {y-h*0.09:.0f}, {w*0.66:.0f} {y+h*0.10:.0f}, "
             f"{w+60} {y-h*0.05:.0f}")
        s += f'<path d="{d}" fill="none" stroke="url(#gh)" stroke-width="{sw}" opacity="{op}"/>'
    return s

def ios_frame(name, headline, sub):
    # App Store 6.5" size (1284 x 2778). Phone prominent up top, bold caption below.
    W, H = 1284, 2778
    s = svg_open(W, H)
    s += premium_bg(W, H)
    scale = 2.34
    sw, sh = 390 * scale, 844 * scale
    px, py = (W - sw) / 2, 172
    bez = 24
    cid = "ph" + name.replace("/", "_").replace("-", "_").replace(".", "_")
    # device body + soft glow
    s += (f'<g filter="url(#sh)"><rect x="{px-bez:.0f}" y="{py-bez:.0f}" width="{sw+2*bez:.0f}" '
          f'height="{sh+2*bez:.0f}" rx="86" fill="#05100d"/></g>')
    s += (f'<clipPath id="{cid}"><rect x="{px:.0f}" y="{py:.0f}" width="{sw:.0f}" '
          f'height="{sh:.0f}" rx="54"/></clipPath>')
    s += (f'<g clip-path="url(#{cid})"><g transform="translate({px:.0f},{py:.0f}) '
          f'scale({scale})">{ios_screen()}</g></g>')
    # subtle screen edge highlight
    s += (f'<rect x="{px:.0f}" y="{py:.0f}" width="{sw:.0f}" height="{sh:.0f}" rx="54" '
          f'fill="none" stroke="rgba(255,255,255,.06)" stroke-width="2"/>')
    # caption at the bottom: subtitle over a bold headline
    cap = py + sh + bez
    s += txt(W // 2, cap + 132, sub, 42, 500, "#8fc7b5", SANS, "middle")
    s += txt(W // 2, cap + 226, headline, 74, 800, "#ffffff", SANS, "middle")
    s += "</svg>"
    render(name, s)

# ---- Ko-fi images ---------------------------------------------------------
def kofi_hero(name, W=1600, H=1000):
    s = svg_open(W, H)
    s += dark_bg(W, H)
    s += mark(W-360, 60, 300, "url(#g)")
    s += txt(90, 300, "Lidhra", 130, 800, "#ffffff", SANS)
    s += txt(96, 380, "Link what matters.", 52, 500, "#bfe9d8", SANS)
    s += txt(96, 470, "A fast, native download app that runs on your", 34, 400, "#8fc7b5", SANS)
    s += txt(96, 516, "own debrid account. macOS · Windows · Linux.", 34, 400, "#8fc7b5", SANS)
    # small window peeking
    scale = 1.0
    s += f'<g transform="translate(96,600) scale(0.86)">{mac_window()}</g>'
    s += "</svg>"
    render(name, s)

def kofi_feature(name, headline, sub, kind):
    W, H = 2000, 1250
    s = svg_open(W, H)
    s += light_bg(W, H)
    s += txt(W//2, 170, headline, 78, 800, INK, SANS, "middle")
    s += txt(W//2, 250, sub, 40, 400, MUTED, SANS, "middle")
    if kind == "window":
        s += f'<g transform="translate({(W-1000*1.5)/2:.0f},340) scale(1.5)">{mac_window()}</g>'
    elif kind == "debrid":
        chips = ["Real-Debrid", "AllDebrid", "TorBox", "Premiumize", "Debrid-Link", "Offcloud"]
        cw, gap = 500, 40
        cols = 3
        gx = (W - (cols*cw + (cols-1)*gap)) / 2
        y = 430
        for i, c in enumerate(chips):
            x = gx + (i % cols) * (cw + gap)
            yy = y + (i // cols) * 150
            s += f'<rect x="{x:.0f}" y="{yy}" width="{cw}" height="110" rx="24" fill="{CARD}" stroke="{BORDER}"/>'
            s += f'<circle cx="{x+58:.0f}" cy="{yy+55}" r="26" fill="url(#g)"/>'
            s += txt(x+104, yy+52, c, 40, 700, INK2)
            s += txt(x+104, yy+84, "connect your account", 24, 400, FAINT, MONO)
        s += txt(W//2, 820, "Torrenting stays on the provider’s cloud, so your device only", 32, 400, MUTED, SANS, "middle")
        s += txt(W//2, 864, "does secure HTTPS. No on-device P2P.", 32, 400, MUTED, SANS, "middle")
    elif kind == "price":
        s += f'<rect x="{(W-760)//2}" y="420" width="760" height="470" rx="36" fill="{CARD}" stroke="{BORDER}" filter="url(#sh)"/>'
        s += mark(W//2-90, 470, 180)
        s += txt(W//2, 720, "7 days free", 66, 800, INK2, SANS, "middle")
        s += txt(W//2, 792, "then €3.99 · yours forever", 40, 500, MUTED, SANS, "middle")
        s += txt(W//2, 852, "one licence · auto-updates · all platforms", 28, 400, FAINT, MONO, "middle")
    s += "</svg>"
    render(name, s)

if __name__ == "__main__":
    print("macOS App Store:")
    macos_frame("appstore/macos/01-overview",
                "Every download, one clean home.",
                "Native sidebar, live health bars, zero clutter.")
    macos_frame("appstore/macos/02-debrid",
                "Bring your own debrid account.",
                "Real-Debrid, AllDebrid, TorBox, Premiumize. Your account, your files.")
    macos_frame("appstore/macos/03-light",
                "Native. Ultralight. ~9 MB.",
                "Built in Rust on the system webview. No Chromium, no bloat.", dark=True)

    print("iOS App Store (1284x2778):")
    ios_frame("appstore/ios/01-transfers", "Your transfers, one tap away",
              "Live health bars, speeds and ETAs at a glance.")
    ios_frame("appstore/ios/02-debrid", "Bring your own debrid",
              "Real-Debrid, AllDebrid, TorBox, Premiumize.")
    ios_frame("appstore/ios/03-private", "Private HTTPS downloads",
              "The cloud does the work. Your device just downloads.")
    ios_frame("appstore/ios/04-native", "Native, light, and yours",
              "English, Greek, Albanian. A one-time purchase.")

    print("Ko-fi:")
    kofi_hero("kofi/00-cover")
    kofi_feature("kofi/01-overview", "One clean home for every download.",
                 "Native sidebar · live health bars · built in Rust.", "window")
    kofi_feature("kofi/02-debrid", "Bring your own debrid account.",
                 "Real-Debrid · AllDebrid · TorBox · Premiumize and more.", "debrid")
    kofi_feature("kofi/03-price", "7-day trial, then yours.",
                 "One licence, auto-updates, every platform.", "price")
    print("done.")
