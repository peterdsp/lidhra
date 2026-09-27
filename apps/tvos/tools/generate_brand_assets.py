#!/usr/bin/env python3
"""Generate the tvOS brand assets (layered app icon, Top Shelf images, in-app mark)
into Lidhra/Assets.xcassets from the Lidhra brand geometry in design/Brand.

The mark is re-rasterised from the two bezier strokes in
design/Brand/svg/lidhra-mark-gradient.svg, so every size is crisp. Text uses
Arial Bold, the font the brand lockup SVG names.

Usage: python3 apps/tvos/tools/generate_brand_assets.py   (needs Pillow)
"""
import json
import os
import shutil

from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
TVOS = os.path.dirname(HERE)
ROOT = os.path.dirname(os.path.dirname(TVOS))
ASSETS = os.path.join(TVOS, "Lidhra", "Assets.xcassets")
BRAND_PNG = os.path.join(ROOT, "design", "Brand", "png")

BG = (7, 17, 15)            # #07110F, the dark app-icon plate
GLOW = (24, 216, 143)       # #18D88F, the app accent
TEXT = (243, 251, 247)      # --text-primary (dark theme)
TAGLINE = (47, 209, 145)    # #2FD191
STOPS = [(0.0, (0x15, 0xC3, 0xB6)), (0.56, (0x2F, 0xD1, 0x91)), (1.0, (0x54, 0xE0, 0x6A))]

# The mark, in its 1024x1024 design space (stroke width 112, round caps).
PATHS = [
    [(348, 258), (214, 315), (210, 510), (326, 594), (414, 658), (490, 618), (550, 558)],
    [(676, 766), (810, 709), (814, 514), (698, 430), (610, 366), (534, 406), (474, 466)],
]
STROKE = 112
MARK_BOX = (187, 202, 837, 822)  # painted bounds of the mark, stroke included

FONT_BOLD = "/System/Library/Fonts/Supplemental/Arial Bold.ttf"
INFO = {"author": "xcode", "version": 1}


def bezier(p0, p1, p2, p3, n=200):
    out = []
    for i in range(n + 1):
        t = i / n
        a, b, c, d = (1 - t) ** 3, 3 * (1 - t) ** 2 * t, 3 * (1 - t) * t ** 2, t ** 3
        out.append((a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0], a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1]))
    return out


def gradient_colour(t):
    t = max(0.0, min(1.0, t))
    for (o0, c0), (o1, c1) in zip(STOPS, STOPS[1:]):
        if t <= o1:
            f = (t - o0) / (o1 - o0)
            return tuple(round(c0[i] + (c1[i] - c0[i]) * f) for i in range(3))
    return STOPS[-1][1]


def stroke_gradient(w, h, gx0, gy0, gw, gh):
    """The SVG's #brand gradient in objectBoundingBox units: t = (u + v) / 2 over the
    path's geometry box, clamped to the end colours outside it. t is linear in x and y,
    so a coarse grid resized bilinearly is exact apart from the clamp."""
    n = 96
    small = Image.new("RGB", (n, n))
    px = small.load()
    for j in range(n):
        for i in range(n):
            x, y = i * (w - 1) / (n - 1), j * (h - 1) / (n - 1)
            px[i, j] = gradient_colour(((x - gx0) / gw + (y - gy0) / gh) / 2)
    return small.resize((w, h), Image.BILINEAR)


def render_mark(size):
    """The mark cropped to its painted bounds, `size` px tall, RGBA."""
    ss = 4
    bx0, by0, bx1, by1 = MARK_BOX
    scale = size * ss / (by1 - by0)
    w, h = round((bx1 - bx0) * scale), round((by1 - by0) * scale)
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    for path in PATHS:
        pts = bezier(*path[0:4]) + bezier(*path[3:7])[1:]
        pts = [((x - bx0) * scale, (y - by0) * scale) for x, y in pts]
        # Stamp round discs along the curve (a round-capped, round-joined stroke).
        # Spacing r/10 keeps the scallop depth under a quarter pixel after downsampling.
        mask = Image.new("L", (w, h), 0)
        d = ImageDraw.Draw(mask)
        r = STROKE * scale / 2
        step, run = r / 10, float("inf")
        for (x0, y0), (x1, y1) in zip(pts, pts[1:] + [pts[-1]]):
            if run >= step or (x0, y0) == pts[-1]:
                d.ellipse((x0 - r, y0 - r, x0 + r, y0 + r), fill=255)
                run = 0.0
            run += ((x1 - x0) ** 2 + (y1 - y0) ** 2) ** 0.5
        # Gradient box = the path geometry (without stroke), as SVG does for objectBoundingBox.
        xs = [(p[0] - bx0) * scale for p in path]
        ys = [(p[1] - by0) * scale for p in path]
        layer = stroke_gradient(w, h, min(xs), min(ys), max(xs) - min(xs), max(ys) - min(ys))
        stroke = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        stroke.paste(layer, (0, 0), mask)
        out = Image.alpha_composite(out, stroke)
    return out.resize((w // ss, h // ss), Image.LANCZOS)


def background(w, h, glow=0.22):
    """Opaque brand plate: #07110F with a soft accent glow off-centre."""
    img = Image.new("RGB", (w, h), BG)
    glow_layer = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(glow_layer)
    r = int(min(w, h) * 0.75)
    cx, cy = int(w * 0.62), int(h * 0.30)
    d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=int(255 * glow))
    glow_layer = glow_layer.filter(ImageFilter.GaussianBlur(min(w, h) * 0.30))
    img.paste(Image.new("RGB", (w, h), GLOW), (0, 0), glow_layer)
    return img


def centred(canvas_size, art, dy=0):
    w, h = canvas_size
    layer = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    layer.paste(art, ((w - art.width) // 2, (h - art.height) // 2 + dy), art)
    return layer


def tracked_text(text, font, tracking):
    widths = [font.getlength(ch) for ch in text]
    total = sum(widths) + tracking * (len(text) - 1)
    asc, desc = font.getmetrics()
    img = Image.new("L", (int(total) + 4, asc + desc), 0)
    d = ImageDraw.Draw(img)
    x = 0.0
    for ch, cw in zip(text, widths):
        d.text((x, 0), ch, font=font, fill=255)
        x += cw + tracking
    return img.crop(img.getbbox())


def lockup(height):
    """Mark + LIDHRA + tagline, laid out like the horizontal brand lockup, `height` px tall."""
    mark = render_mark(height)
    title_font = ImageFont.truetype(FONT_BOLD, int(height * 0.50))
    tag_font = ImageFont.truetype(FONT_BOLD, int(height * 0.13))
    title = tracked_text("LIDHRA", title_font, height * 0.09)
    tag = tracked_text("LINK WHAT MATTERS", tag_font, height * 0.05)
    gap = int(height * 0.28)
    text_w = max(title.width, tag.width)
    w = mark.width + gap + text_w
    out = Image.new("RGBA", (w, height), (0, 0, 0, 0))
    out.paste(mark, (0, 0), mark)
    tx = mark.width + gap
    block_h = title.height + int(height * 0.14) + tag.height
    ty = (height - block_h) // 2
    out.paste(Image.new("RGBA", title.size, TEXT + (255,)), (tx, ty), title)
    out.paste(Image.new("RGBA", tag.size, TAGLINE + (255,)), (tx + 2, ty + title.height + int(height * 0.14)), tag)
    return out


def write_json(path, obj):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(obj, f, indent=2)
        f.write("\n")


def save(img, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    img.save(path, optimize=True)


def imageset(folder, images):
    """images: [(filename, scale, PIL image)]"""
    for name, _, img in images:
        save(img, os.path.join(folder, name))
    write_json(os.path.join(folder, "Contents.json"), {
        "images": [{"filename": n, "idiom": "tv", "scale": s} for n, s, _ in images],
        "info": INFO,
    })


def imagestack(folder, sizes, scales):
    """A two-layer parallax stack (Front = mark, Back = brand plate)."""
    w1, h1 = sizes
    front, back = [], []
    for s in scales:
        w, h = w1 * s, h1 * s
        back.append((f"back@{s}x.png", f"{s}x", background(w, h)))
        front.append((f"front@{s}x.png", f"{s}x", centred((w, h), render_mark(int(h * 0.58)))))
    for layer, imgs in (("Front", front), ("Back", back)):
        lf = os.path.join(folder, f"{layer}.imagestacklayer")
        imageset(os.path.join(lf, "Content.imageset"), imgs)
        write_json(os.path.join(lf, "Contents.json"), {"info": INFO})
    write_json(os.path.join(folder, "Contents.json"), {
        "info": INFO,
        "layers": [{"filename": "Front.imagestacklayer"}, {"filename": "Back.imagestacklayer"}],
    })


def top_shelf(w, h):
    img = background(w, h, glow=0.16).convert("RGBA")
    art = lockup(int(h * 0.36))
    return Image.alpha_composite(img, centred((w, h), art)).convert("RGB")


def main():
    brand = os.path.join(ASSETS, "App Icon & Top Shelf Image.brandassets")
    shutil.rmtree(brand, ignore_errors=True)
    imagestack(os.path.join(brand, "App Icon.imagestack"), (400, 240), [1, 2])
    imagestack(os.path.join(brand, "App Icon - App Store.imagestack"), (1280, 768), [1])
    imageset(os.path.join(brand, "Top Shelf Image.imageset"),
             [("top-shelf@1x.png", "1x", top_shelf(1920, 720)), ("top-shelf@2x.png", "2x", top_shelf(3840, 1440))])
    imageset(os.path.join(brand, "Top Shelf Image Wide.imageset"),
             [("top-shelf-wide@1x.png", "1x", top_shelf(2320, 720)),
              ("top-shelf-wide@2x.png", "2x", top_shelf(4640, 1440))])
    write_json(os.path.join(brand, "Contents.json"), {
        "assets": [
            {"filename": "App Icon - App Store.imagestack", "idiom": "tv", "role": "primary-app-icon", "size": "1280x768"},
            {"filename": "App Icon.imagestack", "idiom": "tv", "role": "primary-app-icon", "size": "400x240"},
            {"filename": "Top Shelf Image Wide.imageset", "idiom": "tv", "role": "top-shelf-image-wide", "size": "2320x720"},
            {"filename": "Top Shelf Image.imageset", "idiom": "tv", "role": "top-shelf-image", "size": "1920x720"},
        ],
        "info": INFO,
    })

    # In-app mark (Connect screen, headers): 160pt tall.
    mark = os.path.join(ASSETS, "BrandMark.imageset")
    shutil.rmtree(mark, ignore_errors=True)
    imageset(mark, [("mark@1x.png", "1x", render_mark(160)), ("mark@2x.png", "2x", render_mark(320))])

    write_json(os.path.join(ASSETS, "AccentColor.colorset", "Contents.json"), {
        "colors": [{"color": {"color-space": "srgb", "components": {
            "red": "0x18", "green": "0xD8", "blue": "0x8F", "alpha": "1.000"}}, "idiom": "universal"}],
        "info": INFO,
    })
    # Launch screen colour (Config/Info.plist UILaunchScreen), the app background #07110E.
    write_json(os.path.join(ASSETS, "LaunchBackground.colorset", "Contents.json"), {
        "colors": [{"color": {"color-space": "srgb", "components": {
            "red": "0x07", "green": "0x11", "blue": "0x0E", "alpha": "1.000"}}, "idiom": "universal"}],
        "info": INFO,
    })
    write_json(os.path.join(ASSETS, "Contents.json"), {"info": INFO})
    print("Brand assets written to", ASSETS)


if __name__ == "__main__":
    main()
