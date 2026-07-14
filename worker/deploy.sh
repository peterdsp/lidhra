#!/usr/bin/env bash
# One-shot deploy for the Lidhra licence Worker.
#
# Auth (pick one, done once):
#   npx wrangler login                      # interactive, opens Cloudflare
#   export CLOUDFLARE_API_TOKEN=<token>     # headless; token needs
#                                           #   Account > Workers Scripts: Edit
#                                           #   Account > Workers KV Storage: Edit
#                                           #   Account > Account Settings: Read
#
# Usage:
#   ./deploy.sh <KOFI_VERIFICATION_TOKEN> [RESEND_API_KEY]
#
# Reads the issuer PRIVATE key from ~/.lidhra-keys/issuer.txt (never printed).
set -euo pipefail
cd "$(dirname "$0")"

KOFI_TOKEN="${1:-}"
RESEND_KEY="${2:-}"
if [ -z "$KOFI_TOKEN" ]; then
  echo "usage: ./deploy.sh <KOFI_VERIFICATION_TOKEN> [RESEND_API_KEY]" >&2
  exit 2
fi

ISSUER_SEED="$(grep -i private ~/.lidhra-keys/issuer.txt | grep -oE '[0-9a-f]{64}')"
if [ -z "$ISSUER_SEED" ]; then
  echo "could not read 64-hex issuer key from ~/.lidhra-keys/issuer.txt" >&2
  exit 1
fi

command -v wrangler >/dev/null 2>&1 || alias wrangler="npx wrangler"
WR="npx wrangler"

echo "==> ensuring deps"
[ -d node_modules ] || npm install --no-audit --no-fund

echo "==> creating KV namespace LICENSES (idempotent)"
KV_OUT="$($WR kv namespace create LICENSES 2>&1 || true)"
echo "$KV_OUT"
KV_ID="$(printf '%s' "$KV_OUT" | grep -oE 'id = "[0-9a-f]{32}"' | grep -oE '[0-9a-f]{32}' | head -1)"
if [ -n "$KV_ID" ]; then
  # patch wrangler.toml with the real id
  sed -i.bak "s/REPLACE_WITH_KV_ID/$KV_ID/" wrangler.toml && rm -f wrangler.toml.bak
  echo "    KV id: $KV_ID (written to wrangler.toml)"
else
  echo "    (KV already existed or id not parsed; make sure wrangler.toml has the id)"
fi

echo "==> setting secrets"
printf '%s' "$ISSUER_SEED" | $WR secret put ISSUER_SEED_HEX
printf '%s' "$KOFI_TOKEN"  | $WR secret put KOFI_TOKEN
if [ -n "$RESEND_KEY" ]; then printf '%s' "$RESEND_KEY" | $WR secret put RESEND_API_KEY; fi

echo "==> deploying"
$WR deploy

echo
echo "Done. Set your Ko-fi webhook (More > API > Webhooks) to  <printed-url>/kofi"
echo "If the printed URL is not https://lidhra-license.peterdsp.workers.dev,"
echo "set LIDHRA_ACTIVATE_URL in the app or update the constant and rebuild."
