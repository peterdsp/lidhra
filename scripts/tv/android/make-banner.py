#!/usr/bin/env python3
"""Render the Android TV launcher banner from the brand assets.

Outputs (committed, so a build never needs Pillow):
  app/src-tauri/icons/android-tv/tv_banner.png        320x180, xhdpi launcher banner
  app/src-tauri/icons/android-tv/tv_banner_1280x720.png  Google Play TV banner

Design: dark brand background (#07110F), the gradient Lidhra mark on the left,
the LIDHRA wordmark in white and the tagline in brand green, sized to stay
legible from across a room. Everything is drawn at 1280x720 and scaled down
with LANCZOS so the small banner stays crisp.

Usage: python3 scripts/tv/android/make-banner.py
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[3]
BRAND = ROOT / "design" / "Brand" / "png"
OUT = ROOT / "app" / "src-tauri" / "icons" / "android-tv"

BG = (7, 17, 15)          # #07110F, the brand ink colour
BG_GLOW = (12, 38, 33)    # subtle teal lift behind the mark
WHITE = (247, 251, 249)   # #F7FBF9
GREEN = (47, 209, 145)    # #2FD191

FONT_CANDIDATES = [
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/Library/Fonts/Arial Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
]


def font(size: int) -> ImageFont.FreeTypeFont:
    for path in FONT_CANDIDATES:
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    raise SystemExit("make-banner: no bold TrueType font found (tried %s)" % FONT_CANDIDATES)


def spaced_text(draw, xy, text, fnt, fill, tracking):
    """Draw text with extra letter spacing, return the right edge x."""
    x, y = xy
    for ch in text:
        draw.text((x, y), ch, font=fnt, fill=fill)
        x += draw.textlength(ch, font=fnt) + tracking
    return x - tracking


def spaced_width(draw, text, fnt, tracking):
    return sum(draw.textlength(ch, font=fnt) for ch in text) + tracking * (len(text) - 1)


def render(w: int = 1280, h: int = 720) -> Image.Image:
    img = Image.new("RGB", (w, h), BG)

    # Soft radial glow on the left, behind the mark.
    glow = Image.new("L", (w, h), 0)
    gd = ImageDraw.Draw(glow)
    cx, cy, r = int(w * 0.24), h // 2, int(h * 0.62)
    for i in range(r, 0, -4):
        gd.ellipse((cx - i, cy - i, cx + i, cy + i), fill=int(255 * (1 - i / r) ** 1.6))
    img.paste(Image.new("RGB", (w, h), BG_GLOW), (0, 0), glow)

    mark = Image.open(BRAND / "lidhra-mark-gradient-1024.png").convert("RGBA")
    mark = mark.crop(mark.getbbox())
    mh = int(h * 0.52)
    mark = mark.resize((int(mark.width * mh / mark.height), mh), Image.LANCZOS)

    draw = ImageDraw.Draw(img)
    word_f = font(int(h * 0.205))
    tag_f = font(int(h * 0.056))
    word, tag = "LIDHRA", "LINK WHAT MATTERS"
    word_track, tag_track = int(h * 0.025), int(h * 0.018)
    ww = spaced_width(draw, word, word_f, word_track)
    tw = spaced_width(draw, tag, tag_f, tag_track)

    gap = int(w * 0.045)
    total = mark.width + gap + max(ww, tw)
    x0 = int((w - total) / 2)
    img.paste(mark, (x0, (h - mh) // 2), mark)

    tx = x0 + mark.width + gap
    wa, wd = word_f.getmetrics()
    ta, td = tag_f.getmetrics()
    block = wa + int(h * 0.04) + ta
    ty = (h - block) // 2 - int(h * 0.02)
    spaced_text(draw, (tx, ty), word, word_f, WHITE, word_track)
    spaced_text(draw, (tx + 4, ty + wa + int(h * 0.04)), tag, tag_f, GREEN, tag_track)
    return img


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    big = render(1280, 720)
    big.save(OUT / "tv_banner_1280x720.png", optimize=True)
    big.resize((320, 180), Image.LANCZOS).save(OUT / "tv_banner.png", optimize=True)
    print("make-banner: wrote", OUT / "tv_banner.png", "and", OUT / "tv_banner_1280x720.png")


if __name__ == "__main__":
    main()
