#!/usr/bin/env bash
# ADR 0032 (installer lifecycle A2): a throwaway code-signing identity for the two macOS update-fixture
# builds on a disposable GitHub-hosted runner, so A and B share one designated requirement as a
# Developer ID release would, and the Keychain lets B read the data key A created.
#
#   bash scripts/update/macos-disposable-signing.sh create   # writes APPLE_SIGNING_IDENTITY to $GITHUB_ENV
#   bash scripts/update/macos-disposable-signing.sh delete   # removes the keychain and the trust setting
#
# Self-signed, valid for 2 days, generated on the runner and never uploaded. The private key exists only
# inside a temporary keychain with a random password. Not a release identity: packages signed with it are
# internal test builds, never distributed. Refuses to run outside a GitHub-hosted macOS runner.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" || "${GITHUB_ACTIONS:-}" != "true" || "${RUNNER_ENVIRONMENT:-}" != "github-hosted" ]]; then
  echo "macos-disposable-signing: refused (not a GitHub-hosted macOS runner)" >&2
  exit 2
fi

# Runs a command for at most $1 seconds (macOS has no GNU timeout). A `security` call that waits for a
# dialog no one can answer on a runner is killed and fails (exit 137) instead of hanging the job.
bounded() {
  local secs="$1" pid watcher rc
  shift
  "$@" &
  pid=$!
  (sleep "$secs" && kill -9 "$pid") >/dev/null 2>&1 &
  watcher=$!
  rc=0
  wait "$pid" || rc=$?
  kill "$watcher" 2>/dev/null || true
  return "$rc"
}

work="${RUNNER_TEMP:?}/downova-disposable-signing"
kc="$work/signing.keychain-db"
cn="Downova CI disposable test signing"

case "${1:-}" in
create)
  rm -rf "$work"
  mkdir -p "$work"
  chmod 700 "$work"
  pw="$(/usr/bin/openssl rand -hex 24)"
  cat >"$work/cert.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $cn
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF
  # /usr/bin/openssl (LibreSSL) writes a PKCS#12 that `security import` reads.
  /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 2 -config "$work/cert.cnf" \
    -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
  /usr/bin/openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" \
    -name "$cn" -out "$work/id.p12" -passout "pass:$pw"
  rm -f "$work/key.pem"
  bounded 60 security create-keychain -p "$pw" "$kc"
  bounded 60 security set-keychain-settings -lut 21600 "$kc"
  bounded 60 security unlock-keychain -p "$pw" "$kc"
  bounded 60 security import "$work/id.p12" -k "$kc" -P "$pw" -T /usr/bin/codesign
  rm -f "$work/id.p12"
  bounded 60 security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$pw" "$kc" >/dev/null
  # Searched for identities; the login keychain stays the default (the service's data key lives there).
  existing="$(security list-keychains -d user | tr -d '"' | xargs)"
  # shellcheck disable=SC2086
  bounded 60 security list-keychains -d user -s "$kc" $existing
  # Trust for code signing on this runner only (best effort, never waits for a password or dialog;
  # codesign also signs with an untrusted certificate).
  bounded 60 sudo -n security add-trusted-cert -d -r trustRoot -p codeSign \
    -k /Library/Keychains/System.keychain "$work/cert.pem" ||
    echo "macos-disposable-signing: trust setting not added; continuing untrusted"
  hash="$(bounded 60 security find-identity -p codesigning "$kc" | awk -v cn="$cn" 'index($0, cn) {print $2; exit}')"
  if [[ -z "$hash" ]]; then
    security find-identity -p codesigning "$kc" >&2 || true
    echo "macos-disposable-signing: identity not found" >&2
    exit 1
  fi
  echo "macos-disposable-signing: identity $hash (certificate SHA-256 $(/usr/bin/openssl x509 -in "$work/cert.pem" -noout -fingerprint -sha256 | cut -d= -f2))"
  echo "APPLE_SIGNING_IDENTITY=$hash" >>"${GITHUB_ENV:?}"
  ;;
delete)
  # The trust setting is left: removing it can wait for an authorization no one gives on a runner
  # (installer lifecycle 42602432329 hung here for two hours), and the runner is discarded after the job.
  # The keychain holding the private key is deleted.
  bounded 60 security delete-keychain "$kc" 2>/dev/null || true
  rm -rf "$work"
  echo "macos-disposable-signing: keychain and files removed (runner trust setting left; the runner is discarded)"
  ;;
*)
  echo "usage: macos-disposable-signing.sh create|delete" >&2
  exit 2
  ;;
esac
