# lidhra-debrid

Unified async interface over debrid / multi-hoster services for **Lidhra**.

The rest of the app depends only on the `DebridProvider` trait - adding a provider is one adapter.
**Real-Debrid is fully wired** (real REST v1.0 endpoints); AllDebrid, TorBox, Premiumize, Debrid-Link,
Offcloud, Mega-Debrid, Deepbrid and High-Way follow the same shape.

Why this exists: it's the layer that lets Lidhra ship on the App Store. The device sends a magnet to a
debrid cloud service, which torrents it on **its** servers and returns a **direct HTTPS (TLS) link** -
so the device only ever does an encrypted download, never on-device P2P. See
`../../Strategy/DEBRID-AND-APPSTORE-STRATEGY.md`.

## The flow

```text
magnet / hash / .torrent
   │  check_cache()   which providers already have it (instant)?
   ▼  add_magnet()    provider torrents it in the cloud - no P2P on device
   ▼  transfer()      poll until Ready
   ▼  unrestrict()    restricted link → direct HTTPS URL (TLS)
   ▼  → hand DirectLink to Lidhra's HTTPS download engine
```

## Try it (real Real-Debrid account)

```sh
export RD_TOKEN=your_real_debrid_api_token   # my.real-debrid.com/apitoken
cargo run --example resolve_magnet -- "magnet:?xt=urn:btih:...&dn=ubuntu-24.04.iso"
```

Use a **legitimate** magnet (a Linux ISO, a Creative-Commons film, your own file) - this is exactly the
flow to record for the App Review evidence pack.

## Layout

| File | Purpose |
|------|---------|
| `src/provider.rs` | the `DebridProvider` trait |
| `src/model.rs` | shared models + magnet/info-hash parsing (hex & base32) |
| `src/policy.rs` | `resolve()` - cache-probe, rank, add, fail over across providers |
| `src/providers/real_debrid.rs` | working Real-Debrid adapter |
| `examples/resolve_magnet.rs` | end-to-end demo |

## Status

- ✅ Trait, models, policy engine, unit tests, runnable example.
- ✅ Adapters: **Real-Debrid** (fully wired + tested), **AllDebrid**, **TorBox**, **Premiumize**, **Debrid-Link**, **Offcloud**, **Mega-Debrid**, **Deepbrid**, **High-Way** (implemented against their public APIs or reference clients - verify field paths against a live account).
- ✅ Registry: `build_provider(id, credential)` + `ProviderId::from_key("torbox")` + `ProviderId::IMPLEMENTED` - pick any provider by name (used by the CLI's `--provider` and the future settings UI).
- ✅ Wired to `lidhra-transfer` via the `lidhra` CLI (`../lidhra-cli`): `lidhra add "<magnet>" --provider <name>` → debrid → resumable HTTPS download → file.
- ⬜ OAuth device-code flow helper (Real-Debrid / Premiumize).
- ⬜ Wire into `lidhra-core` (Tauri).

Notes: Real-Debrid deprecated `/torrents/instantAvailability` in 2024, so `check_cache` may report
"not cached" and callers fall back to add + poll - this is intentional and documented in the adapter.

Per-provider credentials and caveats:

| Provider | Credential | Notes |
| --- | --- | --- |
| Real-Debrid | API token or OAuth access token | Cache check deprecated upstream. |
| AllDebrid | API key | Magnet only. |
| TorBox | API key | Restricted links are `torrent_id:file_id`. |
| Premiumize | API key | Folder links are already direct. |
| Debrid-Link | API token (`webapp/apikey`) | Seedbox `downloadUrl`s are direct; other URLs go through `downloader/add`. |
| Offcloud | API key (account page) | Cloud download URLs are direct. Delete uses the web `cloud/remove` route. |
| Mega-Debrid | `login:password`, or a raw token | No long-lived key: the adapter logs in and renews the short-lived token. Token-only mode cannot renew or report premium status. No delete action in the API. |
| Deepbrid | API key (Devices & API page) | Status vocabulary mirrors Real-Debrid. Torrent links go through `generate/link`. |
| High-Way | API token (Center → API token) | No public API reference; follows the transfer API used by torrentio. List and delete are unverified guesses at the same shape. Account call only validates the token. |
