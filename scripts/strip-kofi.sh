#!/usr/bin/env bash
# Remove the Ko-fi trial / license / purchase UI from the shared web UI so an
# App Store build can never expose it (App Store Review Guideline 3.1.1), even if
# a runtime check ever regresses. The direct-download (Ko-fi) build keeps it.
#
# The Ko-fi UI and logic in ui/index.html are wrapped in marker pairs:
#   <!-- KOFI:START ... -->  ...  <!-- KOFI:END -->    (HTML)
#   /* KOFI:START */         ...  /* KOFI:END */       (JS)
# This script deletes everything between (and including) those markers, then
# fails loudly if any active Ko-fi surface remains.
#
# Usage: scripts/strip-kofi.sh [path/to/index.html]   (default: ui/index.html)
set -euo pipefail

UI="${1:-ui/index.html}"
[ -f "$UI" ] || { echo "::error::strip-kofi: $UI not found"; exit 1; }

perl -0777 -i -pe 's/<!-- KOFI:START.*?KOFI:END -->//gs; s{/\* KOFI:START \*/.*?/\* KOFI:END \*/}{}gs' "$UI"

# The license banner element and every ko-fi.com purchase link must be gone.
if grep -Eq 'ko-fi\.com|class="lic"' "$UI"; then
  echo "::error::strip-kofi: Ko-fi UI still present in $UI after strip"
  grep -nE 'ko-fi\.com|class="lic"' "$UI" || true
  exit 1
fi

echo "strip-kofi: removed Ko-fi trial/license/purchase UI from $UI"
