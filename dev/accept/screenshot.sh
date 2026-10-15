#!/usr/bin/env bash
# Acceptance script 'screenshot': a Screenshot portal call through xdg-desktop-portal returns the address of a PNG of the output.
# Needs TRANSOM_COMPOSITOR_ENV: the env file of a headless compositor session; its output is named
# in TRANSOM_OUTPUT_NAME. Needs TRANSOM_BIN.
# Prints PASS or FAIL per step. Calls require_jail first. Owner: P2-T1.
#   exit 0 all steps passed, 1 a step failed, 2 not runnable (outside the jail), 3 a tool is missing.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=dev/lib/jail.sh
source "$root/dev/lib/jail.sh"
require_jail
# shellcheck source=dev/lib/chain.sh
source "$root/dev/lib/chain.sh"
# shellcheck source=dev/lib/cast_check.sh
source "$root/dev/lib/cast_check.sh"
command -v gdbus >/dev/null || { echo "gdbus is not installed" >&2; exit 3; }
failed=0
pass() { echo "PASS $1: $2"; }
fail() { echo "FAIL $1: $2"; failed=1; }
PORTAL=(gdbus call --session --dest org.freedesktop.portal.Desktop --object-path /org/freedesktop/portal/desktop)
: "${TRANSOM_COMPOSITOR_ENV:?set TRANSOM_COMPOSITOR_ENV to the env file of the compositor session}"
start_chain --env "$TRANSOM_COMPOSITOR_ENV"
trap 'stop_fake_shell; stop_chain' EXIT
# The portal frontend asks the Access portal before it takes a screenshot; the fake shell says yes
# (TRANSOM_FAKE_ACCESS, default 0) and the call is logged in $SHELL_LOG.
start_fake_shell none
dir=$(mktemp -d)
mon="$dir/monitor"
gdbus monitor --session --dest org.freedesktop.portal.Desktop >"$mon" 2>&1 &
mpid=$!
trap 'kill "$mpid" 2>/dev/null || true; stop_fake_shell; stop_chain' EXIT
sleep 0.5
"${PORTAL[@]}" --method org.freedesktop.portal.Screenshot.Screenshot "" "{'handle_token': <'accept'>}" >/dev/null
uri=""
for _ in $(seq 1 100); do
  uri=$(grep -o "'uri': <'file://[^']*'>" "$mon" | head -n 1 | sed "s/.*<'file:\/\/\(.*\)'>/\1/" || true)
  [ -n "$uri" ] && break
  sleep 0.1
done
if [ -z "$uri" ]; then fail response "no uri arrived"; else pass response "a uri arrived"; fi
if grep -q "^ask " "$SHELL_LOG"; then pass access "the person was asked through the shell"; else echo "NOTE access: the frontend did not ask (an app it already trusts is not asked)"; fi
file=$(printf '%b' "$(printf '%s' "$uri" | sed 's/%\(..\)/\\x\1/g')")
if [ -f "$file" ] && [ "$(head -c 8 "$file" | od -An -tx1 | tr -d ' \n')" = "89504e470d0a1a0a" ]; then
  pass png "$(basename "$file") is a PNG"
else
  fail png "the uri does not name a PNG"
fi
width=$(od -An -tu1 -j16 -N4 "$file" | awk '{print $1*16777216+$2*65536+$3*256+$4}')
height=$(od -An -tu1 -j20 -N4 "$file" | awk '{print $1*16777216+$2*65536+$3*256+$4}')
echo "size ${width}x${height}"
# The output's size at its scale is whatever the compositor session reports in TRANSOM_OUTPUT_SIZE (WxH).
if [ -n "${TRANSOM_OUTPUT_SIZE:-}" ]; then
  if [ "${width}x${height}" = "$TRANSOM_OUTPUT_SIZE" ]; then pass size "equals the output"; else fail size "expected $TRANSOM_OUTPUT_SIZE"; fi
fi
exit "$failed"
