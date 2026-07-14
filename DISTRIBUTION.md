# Distribution, pricing, trial + license

Lidhra ships in two flavors from one codebase, chosen with a Cargo feature.

| | Ko-fi / direct | App Store (macOS / iOS) |
|---|---|---|
| Feature | `kofi` (default) | `appstore` |
| Price | **€3.99** | **$5.99** |
| Trial | **7 days free**, then a license key | none (paid upfront) |
| License logic | Ed25519 key check (offline) | none (store receipt is the license) |
| Auto-update | **yes** (built-in updater) | no (the store updates it) |
| Build | `cargo tauri build` | `cargo tauri build --no-default-features --features appstore -c src-tauri/tauri.appstore.conf.json` |

The 7-day trial + license engine lives in `crates/lidhra-license` and is verified by tests.
The App Store build compiles the trial/updater out, so there is no self-update and nothing that
conflicts with App Store rules.

## One-time setup (before the first release)

1. **License issuer key** (for selling on Ko-fi):
   ```sh
   cargo run -p lidhra-license --bin lidhra-keygen -- genkey
   ```
   Put the **public** key into `crates/lidhra-license/src/lib.rs` (`ISSUER_PUBKEY_HEX`).
   Keep the **private** key secret (a password manager, not the repo).

2. **Updater signing key** (for auto-update):
   ```sh
   cargo install tauri-cli --version "^2"
   cargo tauri signer generate -w ~/.lidhra-updater.key
   ```
   Put the printed **public** key into `app/src-tauri/tauri.conf.json` -> `plugins.updater.pubkey`.
   Add the **private** key + its password as GitHub repo secrets:
   `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

## Releasing (Ko-fi / direct, all platforms + auto-update)

```sh
git tag v0.1.0 && git push origin v0.1.0
```
`.github/workflows/release.yml` builds signed installers for macOS / Windows / Linux, publishes a
GitHub Release, and generates `latest.json`. Installed apps check that feed on launch and update
themselves. Nothing else to host.

## Selling on Ko-fi

1. Create a Ko-fi product/shop item at €3.99.
2. On each purchase, mint the buyer a key:
   ```sh
   cargo run -p lidhra-license --bin lidhra-keygen -- sign <ISSUER_PRIVATE_HEX> "buyer@email"
   ```
3. Deliver the `LIDHRA-...` key (Ko-fi delivery message / email). The user pastes it when the trial ends.

## App Store ($5.99, no self-update)

- Build the `appstore` flavor with a config that has **no** `plugins.updater` block
  (`src-tauri/tauri.appstore.conf.json`; copy `tauri.conf.json` and delete the updater section).
- Sign, notarize, and submit through Xcode / Transporter with your Apple Developer account.
- Set the price tier to $5.99. Apple handles updates.

## Auto-update: silent, no passwords

The in-app updater downloads and installs in the background and asks the user for nothing. On Windows the
installer is per-user (no admin / UAC). The only thing that makes an OS security prompt appear is an
**unsigned** app, so a truly prompt-free update needs:

- macOS: a Developer ID certificate + notarization (your Apple account). Then Gatekeeper stays silent and
  the updater swaps the app in place with no password.
- Windows: an Authenticode signing certificate (removes SmartScreen warnings).

Unsigned, it still updates, but the OS may warn on first launch. Signing is the fix, not code.

## Anti-piracy (the honest version)

No downloadable native app can be made 100% unpirateable; a determined person can patch the binary.
Ranked by how hard they are to defeat:

1. **App Store build (strongest).** Apple's receipt + DRM makes it effectively unpirateable for normal
   users, and it is the $5.99 tier. Push mainstream users here.
2. **Online activation (recommended for the direct build).** Sell through a platform with a license API,
   such as **Lemon Squeezy**, Gumroad, or Keygen.sh, which auto-delivers keys, limits activations per key,
   and binds them to a machine server-side. Much harder to share than offline keys. Ko-fi has no license
   API, so for real enforcement prefer Lemon Squeezy for the paid build and keep Ko-fi for donations.
   The client hook is ready: `lidhra_license::machine_id()` gives a stable per-machine id to send on
   activation. Point the app at the platform's validate endpoint and I will wire it.
3. **Node-locked offline keys (no server).** The app shows the user their machine id; you mint a key bound
   to it:
   ```sh
   cargo run -p lidhra-license --bin lidhra-keygen -- sign <ISSUER_PRIVATE_HEX> "MACHINE:<id>"
   ```
   That key only works on that machine (`is_valid_for` enforces it). Stronger than plain keys, but the
   user must send you their id first.
4. **Plain offline keys (weakest, easiest).** What ships today: any signed key works anywhere. Honor
   system, fine for a cheap indie app, trivial to share.

My recommendation for "easy for the user AND hard to pirate": **App Store for mainstream + Lemon Squeezy
online activation for the direct build.** Tell me which and I will wire the activation.

## Notes

- Everything above needs **your** accounts/keys: Apple Developer (~$99/yr), a Windows cert, and a license
  platform. The pipeline is wired; it activates once those exist.
- Pricing is as you asked (**$5.99 App Store, €3.99 direct**). Pricing the store higher than direct is the
  usual pattern, since it absorbs Apple's 15-30% cut.
