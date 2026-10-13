#!/usr/bin/env bash
# Stores the macOS signing + notarization secrets that the Release workflow
# needs in the GitHub repo (soham-verma/pingview), straight from local files.
# Values are piped to `gh secret set`; nothing is printed or written to disk
# (except a temporary file that is deleted on exit).
#
# Usage:
#   scripts/setup_release_secrets.sh <developer-id.p12> <AuthKey_XXXX.p8> <issuer-id>
#
#   developer-id.p12  Your "Developer ID Application" certificate exported
#                     from Keychain Access (My Certificates → right-click →
#                     Export…, .p12). You'll be asked for its password in a
#                     macOS dialog.
#   AuthKey_XXXX.p8   App Store Connect API key used for notarization.
#   issuer-id         App Store Connect Issuer ID (UUID).
set -euo pipefail

REPO="soham-verma/pingview"
P12="${1:?path to the Developer ID .p12}"
P8="${2:?path to AuthKey_XXXX.p8}"
ISSUER="${3:?App Store Connect Issuer ID}"

[[ -f "$P12" ]] || { echo "No such file: $P12" >&2; exit 1; }
[[ -f "$P8" && "$P8" == *AuthKey_*.p8 ]] || { echo "Expected an AuthKey_XXXX.p8 file: $P8" >&2; exit 1; }
[[ "$ISSUER" =~ ^[0-9a-fA-F]{8}-([0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}$ ]] || { echo "Issuer ID must be a UUID." >&2; exit 1; }

KEY_ID="$(basename "$P8" .p8)"
KEY_ID="${KEY_ID#AuthKey_}"

# Signing identity and team come from the certificate in your keychain.
IDENTITY="$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)"
[[ -n "$IDENTITY" ]] || { echo "No Developer ID Application certificate in your keychain." >&2; exit 1; }
TEAM_ID="$(sed -n 's/.*(\([A-Z0-9]\{10\}\))$/\1/p' <<<"$IDENTITY")"

# Ask for the .p12 password in a hidden macOS dialog (works without a terminal).
P12_PASSWORD="$(osascript -e 'text returned of (display dialog "Password for the Developer ID .p12 you exported:" default answer "" with hidden answer with title "PingView release secrets")')"

# Check the password opens the .p12 before uploading anything.
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
if ! openssl pkcs12 -in "$P12" -passin fd:3 -nokeys -out "$TMP/check.pem" 3<<<"$P12_PASSWORD" 2>/dev/null \
   && ! openssl pkcs12 -legacy -in "$P12" -passin fd:3 -nokeys -out "$TMP/check.pem" 3<<<"$P12_PASSWORD" 2>/dev/null; then
  echo "That password doesn't open $P12." >&2
  exit 1
fi

set_secret() { gh secret set "$1" -R "$REPO" >/dev/null; echo "  set $1"; }

echo "Setting secrets on $REPO:"
base64 -i "$P12" | set_secret APPLE_CERTIFICATE
printf '%s' "$P12_PASSWORD" | set_secret APPLE_CERTIFICATE_PASSWORD
printf '%s' "$IDENTITY" | set_secret APPLE_SIGNING_IDENTITY
printf '%s' "$TEAM_ID" | set_secret APPLE_TEAM_ID
printf '%s' "$ISSUER" | set_secret APPLE_API_ISSUER
printf '%s' "$KEY_ID" | set_secret APPLE_API_KEY
base64 -i "$P8" | set_secret APPLE_API_KEY_CONTENT
echo "Done. Push a tag (e.g. git tag v0.1.0 && git push origin v0.1.0) to build a release."
