#!/usr/bin/env bash
# B3.7: a disposable Secret Service for the Linux packaged smoke (CI runners only).
#
#   dbus-run-session -- bash scripts/smoke/linux-secret-service.sh <command> [args...]
#
# The release service keeps protected link parts and checkpoints sealed with a data key stored in
# the OS credential store (ADR 0027). A D-Bus session alone provides no store, so the smoke's
# restart rows (H2, H3, K7) need a real one, as on Windows and macOS. This starts gnome-keyring's
# secrets component inside THIS D-Bus session only (unlocked with a random per-run password),
# proves it works with a store / lookup / clear round trip, and then runs the command. If the
# store is not usable it fails explicitly instead of running the smoke without one.
set -euo pipefail

if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  echo "linux-secret-service: no D-Bus session (run under dbus-run-session)" >&2
  exit 2
fi

password="$(head -c 24 /dev/urandom | base64)"
eval "$(printf '%s' "$password" | gnome-keyring-daemon --unlock --components=secrets)"
unset password

probe="downova-ci-$(head -c 8 /dev/urandom | od -An -tx1 | tr -d ' \n')"
printf '%s' "$probe" | secret-tool store --label="Downova CI probe" downova-ci probe
got="$(secret-tool lookup downova-ci probe || true)"
secret-tool clear downova-ci probe || true
left="$(secret-tool lookup downova-ci probe || true)"
if [ "$got" != "$probe" ] || [ -n "$left" ]; then
  echo "::error title=Secret Service::the disposable credential store is not usable (round trip failed)"
  exit 1
fi
echo "linux-secret-service: usable (gnome-keyring secrets in this D-Bus session; store, lookup and clear round trip passed)"

exec "$@"
