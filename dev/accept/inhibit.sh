#!/usr/bin/env bash
# Acceptance script 'inhibit': an Inhibit request is held while its request object lives and released when it is closed.
# Needs TRANSOM_BIN. No compositor. The login manager is not in the jail, so the effect on the
# session is not checked here (the unit tests check it against a fake); this checks the request
# object transom exports at the handle.
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
reply=$("${PORTAL[@]}" --method org.freedesktop.portal.Inhibit.Inhibit "" 8 "{'reason': <'accept'>}")
handle=$(printf '%s' "$reply" | grep -o "/org/freedesktop/portal/desktop/request/[A-Za-z0-9_/]*" | head -n 1)
[ -n "$handle" ] && pass inhibit "request $handle" || fail inhibit "no request handle in: $reply"
backend() { gdbus introspect --session --dest org.freedesktop.impl.portal.desktop.quire --object-path "$handle" 2>&1; }
case "$(backend)" in *org.freedesktop.impl.portal.Request*) pass held "the request object is exported" ;; *) fail held "no request object" ;; esac
gdbus call --session --dest org.freedesktop.portal.Desktop --object-path "$handle" \
  --method org.freedesktop.portal.Request.Close >/dev/null
sleep 0.5
case "$(backend)" in *org.freedesktop.impl.portal.Request*) fail released "the request object is still there" ;; *) pass released "the request object is gone" ;; esac
exit "$failed"
