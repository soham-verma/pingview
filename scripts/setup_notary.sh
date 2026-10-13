#!/usr/bin/env bash
# One-time setup: stores App Store Connect API credentials for notarization
# in the macOS keychain as profile "pingview-notary". Nothing is written to
# disk or to this repo; notarize.sh only refers to the profile name.
#
# Usage: scripts/setup_notary.sh [issuer-id] [keys-folder]
#   issuer-id   Only for team keys: App Store Connect → Users and Access →
#               Integrations → App Store Connect API (the UUID at the top).
#               Omit it for individual keys.
#   keys-folder default: ~/Documents/Dev/Pingnet/keys
set -euo pipefail

PROFILE="pingview-notary"
ISSUER="${1:-}"
KEYS_DIR="${2:-$HOME/Documents/Dev/Pingnet/keys}"

shopt -s nullglob
keys=("$KEYS_DIR"/AuthKey_*.p8)
if (( ${#keys[@]} == 0 )); then
  echo "No AuthKey_*.p8 files in $KEYS_DIR" >&2
  exit 1
fi

# Individual API keys have no Issuer ID; team keys need one. With no Issuer
# ID given, try every key as an individual key.
# The .p8 file's contents start with "MIG…" (base64 of a private key):
# refuse it loudly rather than treating it as a typo.
if [[ "$ISSUER" == MIG* || "$ISSUER" == *"PRIVATE KEY"* || ${#ISSUER} -gt 60 ]]; then
  echo "That looks like private key material (the contents of a .p8 file), not an Issuer ID." >&2
  echo "Don't paste key contents anywhere. If you already did, revoke that key in App Store" >&2
  echo "Connect and create a new one. The Issuer ID is the UUID at the top of the Team Keys page." >&2
  exit 1
fi
if [[ -n "$ISSUER" && ! "$ISSUER" =~ ^[0-9a-fA-F]{8}-([0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}$ ]]; then
  echo "'$ISSUER' isn't an Issuer ID (a 36-character UUID). A 10-character value is a Key ID:" >&2
  echo "run this without arguments to try your keys as individual keys." >&2
  exit 1
fi

for key in "${keys[@]}"; do
  key_id="$(basename "$key" .p8)"
  key_id="${key_id#AuthKey_}"
  args=(--key "$key" --key-id "$key_id")
  kind="individual"
  if [[ -n "$ISSUER" ]]; then
    args+=(--issuer "$ISSUER")
    kind="team"
  fi
  echo "Trying key ${key_id:0:3}… as a $kind key"
  if xcrun notarytool store-credentials "$PROFILE" "${args[@]}" >/dev/null 2>&1; then
    echo "Saved keychain profile '$PROFILE' (key ${key_id:0:3}…). You can now run scripts/notarize.sh"
    exit 0
  fi
done

if [[ -n "$ISSUER" ]]; then
  echo "Apple rejected every key in $KEYS_DIR with that Issuer ID." >&2
else
  echo "Apple rejected every key in $KEYS_DIR as an individual key." >&2
fi
echo "If these are team keys, pass the Issuer ID; otherwise check that one of these keys" >&2
echo "is still Active in App Store Connect (Users and Access → Integrations)." >&2
exit 1
