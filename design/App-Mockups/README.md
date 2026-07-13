# Lidhra - App Designs (all platforms)

Concept UI mockups for **Lidhra**, exported as editable vector + raster assets.
Covers the app interface on every target platform. The brand/logo/research/compliance
sections of the concept page are intentionally excluded - this pack is the **app screens only**.

## Contents

| File | What it is |
|------|-----------|
| `Lidhra-App-Designs.pdf` | All 5 platforms, one per page - vector, print-ready (1050×705 pt) |
| `svg/01-macos.svg` | macOS - Liquid Glass menu-bar panel + main window |
| `svg/02-ios-watchos.svg` | iOS PWA remote + watchOS glance (Liquid Glass) |
| `svg/03-android.svg` | Android - Material 3 / Material You |
| `svg/04-windows.svg` | Windows 11 - Fluent tray flyout + Mica window |
| `svg/05-linux.svg` | Linux - GNOME/libadwaita desktop + phosh mobile |
| `png/*@2x.png` | Same artboards as 2× PNG (2800×1880) |
| `pdf/*.pdf` | Each platform as its own single-page PDF |

## Formats

- **SVG** - true native vector (paths / text / gradients, no rasterized image, no `foreignObject`).
  Opens and is fully editable in Figma, Illustrator, Sketch, Inkscape, or any browser.
- **PDF** - vector, generated from the same SVGs, so it matches 1:1.
- **PNG** - 2× raster for quick drop-in to slides / docs.

## Notes

- All speeds/states are illustrative sample data (Ubuntu / Debian / Arch ISOs - content-neutral).
- Colors follow the Lidhra tokens: brand teal→green `#15C3B6 → #2FD191 → #54E06A`;
  download `#22BD7A`, seeding `#11A594`, paused `#8A94A8`.
- Fonts render with the system sans/mono at export time; in a design tool, remap to
  SF Pro (Apple), Roboto (Android), Segoe UI Variable (Windows), Cantarell (GNOME) for pixel-accuracy.
- Concept only - independent brand, not affiliated with the qBittorrent project.

Regenerate everything from source with: `python3 build_svgs.py` then the rsvg-convert commands.
