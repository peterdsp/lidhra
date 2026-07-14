# Lidhra marketing images

Store-ready promo art, generated from `build_marketing.py` (brand tokens + a
reusable Lidhra app window / iOS screen). Regenerate with:

```sh
python3 build_marketing.py     # needs rsvg-convert (brew install librsvg)
```

Every image is emitted as both `.svg` (editable source) and `.png` (upload).

## App Store Connect

| Folder | Size | Where it goes |
| --- | --- | --- |
| `appstore/macos/*.png` | 2560 x 1600 | App Store · macOS screenshots (16:10) |
| `appstore/ios/*.png` | 1290 x 2796 | App Store · iPhone 6.7" screenshots |

macOS accepts 1280x800 / 1440x900 / 2560x1600 / 2880x1800. iPhone 6.7" wants
1290x2796. The same shots downscale cleanly for the 6.5" (1284x2778) slot; for
iPad add a 2048x2732 render if you list an iPad build.

## Ko-fi

| File | Use |
| --- | --- |
| `kofi/00-cover.png` | product cover / hero |
| `kofi/01-overview.png` | gallery: the app window |
| `kofi/02-debrid.png` | gallery: bring your own debrid account |
| `kofi/03-price.png` | gallery: trial + price |

Product page: https://ko-fi.com/s/4bbb9c112a

Copy is em-dash-free by house rule. No third-party trademarks are reproduced;
provider names are set as plain text.
