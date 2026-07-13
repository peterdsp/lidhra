# Lidhra complete design system

A production-ready system for **Lidhra** — a cross-platform, native download &amp; transfer manager.
Brand, tokens, components, per-platform screens, developer constants, and the product concept, in one package.

## Start here

- **`index.html`** — the living styleguide. Open it in a browser: foundations, brand, live components,
  every platform screen, and links into every folder. This is the single entry point.

## Main folders

- `Brand/svg` editable master logos, app icons, and lockups
- `Brand/png` raster exports from 16 px through 1024 px
- `Brand/platform-icons` platform-specific source assets and safe-zone notes
- `Design-System/tokens` JSON and CSS design tokens
- `Design-System/docs` logo, colour, naming, and platform usage rules
- `Design-System/previews` colour-language board
- `Developer` SwiftUI, Compose, and Rust colour constants
- `App-Mockups` the cross-platform app screens (SVG · PNG · PDF, plus a combined 5-page PDF)
- `Concept` the full product concept: user research, architecture, and store-compliance strategy
- `Website` the marketing site for **lidhra.peterdsp.dev** — a single self-contained `index.html` (no external assets). Deploy to any static host (Cloudflare Pages, GitHub Pages, Netlify). Design direction: “Peer Mesh” — a live network-graph hero.
- `Strategy` the **debrid integration + App Store compliance** plan — `DEBRID-AND-APPSTORE-STRATEGY.md` (full plan), `debrid-appstore-strategy.html` (deck), and `APP-REVIEW-NOTES.md` (paste-ready App Store Connect review notes + rejection-appeal template). How a debrid architecture lets a native torrent-adjacent client ship legally on the App Store.
- `Code/` a **Cargo workspace** of compiling, tested Rust crates:
  - `lidhra-debrid` — the `DebridProvider` trait + **Real-Debrid** (tested), **AllDebrid**, **TorBox** adapters + policy engine.
  - `lidhra-transfer` — segmented, resumable HTTPS download engine (**verified**: 10 MB over 4 connections, byte-identical to reference).
  - `lidhra-cli` — the `lidhra` binary: `lidhra add "<magnet>" --out <dir>` runs the whole pipeline (magnet → debrid cloud → segmented download → file).
  Build/test all: `cd Code && cargo test`. (`target/` is gitignored — never committed.)

## Recommended production masters

- App Store / iOS: `Brand/platform-icons/apple-ios.svg`
- macOS: `Brand/platform-icons/apple-macos.svg`
- Android adaptive foreground: `Brand/platform-icons/android-foreground.svg`
- Windows Store: `Brand/platform-icons/windows-store.svg`
- Linux desktop/mobile: `Brand/platform-icons/linux.svg`

## Notes

The logo is clean vector geometry — two independent paths that meet, exchange, and continue (from Albanian *lidh*, “to connect”). This gives consistent optical weight at small sizes and avoids image-generation artefacts. The `App-Mockups` screens now use this **same canonical mark** as the brand masters, so the whole package is internally consistent. Colours never drift: `index.html` renders its swatches from the same values defined in `Design-System/tokens`. Regenerate the platform screens with `python3 App-Mockups/build_svgs.py`, then re-render PNG/PDF via `rsvg-convert`. Independent brand — not affiliated with the qBittorrent project.
