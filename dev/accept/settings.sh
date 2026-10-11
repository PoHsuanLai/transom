#!/usr/bin/env bash
# Acceptance script 'settings': ashpd-style reads of org.freedesktop.appearance through the portal, and SettingChanged when the document changes.
# Needs TRANSOM_BIN. No compositor.
# Prints PASS or FAIL per step. Calls require_jail first. Owner: P2-T1.
#   exit 0 all steps passed, 1 a step failed, 2 not runnable (outside the jail), 3 a tool is missing.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=dev/lib/jail.sh
source "$root/dev/lib/jail.sh"
require_jail
# shellcheck source=dev/lib/chain.sh
source "$root/dev/lib/chain.sh"
command -v gdbus >/dev/null || { echo "gdbus is not installed" >&2; exit 3; }
failed=0
pass() { echo "PASS $1: $2"; }
fail() { echo "FAIL $1: $2"; failed=1; }
PORTAL=(gdbus call --session --dest org.freedesktop.portal.Desktop --object-path /org/freedesktop/portal/desktop)
start_chain
trap stop_chain EXIT
conf="$XDG_CONFIG_HOME/quire"
mkdir -p "$conf"
printf '[appearance]\ntheme = "dark"\naccent = "green"\n' > "$conf/appearance.toml"
read_key() { "${PORTAL[@]}" --method org.freedesktop.portal.Settings.Read org.freedesktop.appearance "$1"; }
case "$(read_key color-scheme)" in *"uint32 1"*) pass color-scheme "dark is 1" ;; *) fail color-scheme "$(read_key color-scheme)" ;; esac
case "$(read_key accent-color)" in *"("*) pass accent-color "a colour tuple" ;; *) fail accent-color "$(read_key accent-color)" ;; esac
case "$(read_key contrast)" in *"uint32 0"*) pass contrast "no preference is 0" ;; *) fail contrast "$(read_key contrast)" ;; esac
if "${PORTAL[@]}" --method org.freedesktop.portal.Settings.Read org.freedesktop.appearance nope >/dev/null 2>&1; then
  fail not-found "an unknown key was read"
else
  pass not-found "an unknown key is refused"
fi
mon=$(mktemp)
gdbus monitor --session --dest org.freedesktop.portal.Desktop >"$mon" 2>&1 &
mpid=$!
trap 'kill "$mpid" 2>/dev/null || true; stop_chain' EXIT
sleep 3
printf '[appearance]\ntheme = "light"\naccent = "green"\n' > "$conf/appearance.toml"
for _ in $(seq 1 80); do
  grep -q "SettingChanged.*color-scheme" "$mon" && break
  sleep 0.1
done
if grep -q "SettingChanged.*color-scheme" "$mon"; then pass changed "SettingChanged arrived"; else fail changed "no SettingChanged"; fi
exit "$failed"
