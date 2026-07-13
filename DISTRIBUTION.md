# Distribution, pricing, trial + license

Lidhra ships in two flavors from one codebase, chosen with a Cargo feature.

| | Ko-fi / direct | App Store (macOS / iOS) |
|---|---|---|
| Feature | `kofi` (default) | `appstore` |
| Price | **$4.99** | **$2.99** |
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

1. Create a Ko-fi product/shop item at $4.99.
2. On each purchase, mint the buyer a key:
   ```sh
   cargo run -p lidhra-license --bin lidhra-keygen -- sign <ISSUER_PRIVATE_HEX> "buyer@email"
   ```
3. Deliver the `LIDHRA-...` key (Ko-fi delivery message / email). The user pastes it when the trial ends.

## App Store ($2.99, no self-update)

- Build the `appstore` flavor with a config that has **no** `plugins.updater` block
  (`src-tauri/tauri.appstore.conf.json`; copy `tauri.conf.json` and delete the updater section).
- Sign, notarize, and submit through Xcode / Transporter with your Apple Developer account.
- Set the price tier to $2.99. Apple handles updates.

## Honest notes

- The trial is stored on disk and the license key is not machine-bound, so both are bypassable by a
  determined user. This is a "keep honest people honest" model, normal for indie apps. Add a licensing
  server later if you need hard enforcement.
- Auto-update, code signing, notarization, App Store submission, and Ko-fi all require **your** accounts
  and keys (Apple Developer is ~$99/yr). The pipeline above is wired; it activates once those secrets exist.
- Pricing above is exactly as requested ($2.99 App Store, $4.99 Ko-fi). Note most apps price the store
  higher to absorb the 15-30% cut; adjust if you like.
