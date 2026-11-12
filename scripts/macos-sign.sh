#!/usr/bin/env bash
# Signs a Mac .app for release (CI only: it changes the runner's trust settings).
#
#   scripts/macos-sign.sh path/to/Dotlink.app
#
# Without a Developer ID, Dotlink is signed with a self-signed certificate that stays the same
# from release to release. macOS keeps a permission (Accessibility, Local Network, …) for the
# app's designated requirement: signed ad hoc, that is the binary's hash, so every update was a
# new app and lost them all. With the certificate it is the bundle id and the certificate.
#
# The certificate comes from the environment: MACOS_CERT_P12 (the .p12, base64) and
# MACOS_CERT_PASSWORD. Keep the .p12: a new certificate loses everyone's permissions once more.
# Without them (a fork) the app is signed ad hoc.
set -euo pipefail

app="$1"

if [ -z "${MACOS_CERT_P12:-}" ]; then
  echo "No MACOS_CERT_P12: signing ad hoc (permissions won't survive updates)"
  codesign --force --deep -s - "$app"
  exit 0
fi

work="$(mktemp -d)"
keychain="$work/signing.keychain-db"
pass="$(openssl rand -hex 16)"
echo "$MACOS_CERT_P12" | base64 --decode > "$work/cert.p12"

security create-keychain -p "$pass" "$keychain"
security set-keychain-settings "$keychain"
security unlock-keychain -p "$pass" "$keychain"
# codesign looks the identity up in the search list, whatever --keychain says.
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
security import "$work/cert.p12" -k "$keychain" -P "$MACOS_CERT_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "$pass" "$keychain" >/dev/null
# codesign only uses a trusted identity.
security find-certificate -p "$keychain" > "$work/cert.pem"
sudo security add-trusted-cert -d -r trustRoot -p codeSign -k "$keychain" "$work/cert.pem"

identity="$(openssl x509 -in "$work/cert.pem" -noout -fingerprint -sha1 | cut -d= -f2 | tr -d :)"
# No timestamp: it needs Apple's server, which only answers Developer ID signatures.
codesign --force --deep --keychain "$keychain" -s "$identity" "$app"
codesign --verify --deep --strict "$app"
codesign -d -r- "$app" 2>&1 | grep designated

security delete-keychain "$keychain"
rm -rf "$work"
