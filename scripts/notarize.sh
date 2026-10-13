#!/usr/bin/env bash
# Notarizes and staples a signed PingView DMG, using the keychain profile
# created by setup_notary.sh (no credentials in this script).
#
# Usage: scripts/notarize.sh [path/to.dmg]   (default: the latest release DMG)
set -euo pipefail

PROFILE="pingview-notary"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DMG="${1:-$(ls -t "$ROOT"/src-tauri/target/release/bundle/dmg/*.dmg 2>/dev/null | head -1)}"

if [[ -z "$DMG" || ! -f "$DMG" ]]; then
  echo "No DMG found; build one first (npx tauri build --bundles dmg)." >&2
  exit 1
fi

echo "Submitting $(basename "$DMG") to Apple (usually a few minutes)…"
if ! xcrun notarytool submit "$DMG" --keychain-profile "$PROFILE" --wait | tee /tmp/pingview-notary.log; then
  echo "Submission failed. If it's a credentials error, run scripts/setup_notary.sh first." >&2
  exit 1
fi
if ! grep -q "status: Accepted" /tmp/pingview-notary.log; then
  id="$(awk '/^  id:/{print $2; exit}' /tmp/pingview-notary.log)"
  echo "Apple didn't accept it. Details: xcrun notarytool log $id --keychain-profile $PROFILE" >&2
  exit 1
fi

xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"
spctl --assess --type open --context context:primary-signature -v "$DMG"
echo "Done: $DMG is notarized and stapled."
