# lidhra-bridge

One JSON command surface over [`lidhra-debrid`](../lidhra-debrid) for the TV clients.
On a TV Lidhra is a **library + player**: connect a provider, list the cloud
transfers, resolve a ready transfer to direct HTTPS links, play one. Nothing is
downloaded to the TV.

The same `Bridge::call(name, args)` has two front-ends:

| Front-end | Built for | Used by |
| --- | --- | --- |
| `wasm` (wasm-bindgen, `LidhraBridge` JS class) | `wasm32-unknown-unknown` | Samsung Tizen, LG webOS, HarmonyOS and the hosted TV page (`scripts/tv/build-web.sh`) |
| `ffi` (C ABI, `include/lidhra_bridge.h`) | `aarch64-apple-tvos`, `aarch64-apple-tvos-sim` | the Apple TV app (`apps/tvos`, `scripts/tv/build-apple-tv-core.sh`) |

Android / Google / Fire TV do not need it: they run the Tauri app, whose commands
have the same names and shapes.

## Commands

| Command | Args | Result |
| --- | --- | --- |
| `providers` | | `[{id, label, device_login}]` |
| `connect` | `{provider, token}` | `{username, premium, provider, session}` |
| `login_start` | `{provider}` | `{user_code, verify_url, interval, expires_in, handle}` |
| `login_poll` | `{provider, handle}` | `{pending: true}` or `{pending: false, username, premium, provider, session}` |
| `restore` | `{session}` | same as `connect`, or `null` without a session |
| `session` | | the current session or `null` |
| `disconnect` | | `null` |
| `transfers` | | `[Tx]` (same shape as the desktop `transfers` command) |
| `links` | `{id}` | `[{url, filename, size, mime}]` |

`login_*` is the TV sign-in: the screen shows a short code and URL, the user
approves on a phone. Real-Debrid (OAuth device code) and AllDebrid (PIN) support
it; the other providers take a pasted API key (see `lidhra_debrid::device_auth`).

The host stores the `session` value it gets back (localStorage on web TVs, the
Keychain on Apple TV) and passes it to `restore` on the next launch. A Real-Debrid
device login carries OAuth refresh material; the bridge refreshes the access token
on `restore` and after an auth error mid-session.

## CORS on web TVs

The wasm module calls provider APIs with `fetch`. Tizen widgets declare
`<access origin="*">`, which exempts them from CORS. On webOS, HarmonyOS
(`resource://` origin) and in a plain TV browser the provider must send CORS
headers. Real-Debrid does (verified from a browser on 2026-09-27: the device-code
request succeeds). Providers that do not will fail with a network error on those
platforms; the Tauri (Android TV) and Apple TV builds are not affected.

## Build

```sh
cargo test -p lidhra-bridge                                   # native tests (dispatcher + C ABI)
scripts/tv/build-web.sh                                       # wasm bundle -> build/tv/web
scripts/tv/build-apple-tv-core.sh                             # xcframework -> apps/tvos
```
