# Lidhra license worker

A Cloudflare Worker that makes licensing hands-off. It does two things:

1. **`POST /kofi`** — Ko-fi Shop-Order webhook. Verifies the token, records the
   buyer's email as purchased (in KV), and emails them a short "open the app and
   activate" note. No licence key is ever emailed, so nothing shareable leaks.
2. **`POST /activate`** — called by the Lidhra app with `{ email, machine_id }`.
   If that email purchased and is under the device cap, it mints a **node-locked**
   Ed25519 key (subject `MACHINE:<id>`) that only works on that machine, and
   returns `{ key }`. The app installs it automatically.

The minted key is byte-identical to what `lidhra-keygen sign` produces, so it
verifies against the public key already embedded in `crates/lidhra-license`
(`ISSUER_PUBKEY_HEX`). Ed25519 is deterministic; this was checked directly.

## Go live (about 5 minutes, needs your Cloudflare login)

```sh
cd worker
npm install
npx wrangler login

# 1) create the KV namespace, paste the printed id into wrangler.toml
npx wrangler kv namespace create LICENSES

# 2) set the three secrets (they never leave your machine / Cloudflare)
npx wrangler secret put ISSUER_SEED_HEX   # the 64-hex PRIVATE key from ~/.lidhra-keys/issuer.txt
npx wrangler secret put KOFI_TOKEN        # Ko-fi > More > API > Webhooks: the Verification Token
npx wrangler secret put RESEND_API_KEY    # optional: a Resend key to send the activation email

# 3) deploy
npx wrangler deploy
```

`wrangler deploy` prints the Worker URL, e.g.
`https://lidhra-license.<your-subdomain>.workers.dev`.

Then two small hookups:

- **Ko-fi**: More → API → Webhooks → set the Webhook URL to `<worker-url>/kofi`.
- **App**: if your Worker URL differs from the default
  `https://lidhra-license.peterdsp.workers.dev`, set `LIDHRA_ACTIVATE_URL` (the
  app reads it) or change the constant in `crates/lidhra-server/src/main.rs` and
  `app/src-tauri/src/lib.rs`, then rebuild.

## Anti-piracy

- Keys minted for the app are node-locked (`MACHINE:<id>`) — they do not work on
  another machine (`lidhra-license::is_valid_for`).
- Each purchase can activate up to `MAX_ACTIVATIONS` devices (default 3),
  tracked in KV; further devices are refused.
- The private issuer key lives only as a Worker secret. Losing
  `~/.lidhra-keys/issuer.txt` means you cannot mint keys — keep a backup.

## Test locally

```sh
npx wrangler dev
# then POST /activate with a machine_id after seeding a buyer key in KV
```
