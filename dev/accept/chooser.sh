#!/usr/bin/env bash
# Acceptance script 'chooser': the source chooser handshake: a fake shell answers org.quire.Shell1.PickCaptureSource.
# Prints PASS, SKIP or FAIL per step. Calls require_jail first.
#
# Owner: P2-T2 (chooser client).
#   exit 0 all steps passed, 1 a step failed, 2 not runnable (outside the jail, or not filled yet),
#   3 a tool is missing.
#
# Needs TRANSOM_BIN and TRANSOM_COMPOSITOR_ENV (a headless compositor with an output named in
# $TRANSOM_OUTPUT, default HEADLESS-1).
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=dev/lib/jail.sh
source "$root/dev/lib/jail.sh"
require_jail
# shellcheck source=dev/lib/chain.sh
source "$root/dev/lib/chain.sh"
# shellcheck source=dev/lib/cast_check.sh
source "$root/dev/lib/cast_check.sh"
need_compositor chooser
output=${TRANSOM_OUTPUT:-HEADLESS-1}

env_file=$("$root/dev/up.sh" --env "$TRANSOM_COMPOSITOR_ENV")
# shellcheck source=/dev/null
source "$env_file"
trap 'stop_cast; kill "${SHELL_PID:-0}" 2>/dev/null || true; "$root/dev/down.sh" "$env_file" >/dev/null' EXIT
status=0

# No shell on the bus: sharing is refused, never granted without asking.
run_cast 1 0
if [ "$(cast_field response)" != 0 ]; then step PASS "1 without a shell the share is refused"; else step FAIL "1 a share was granted with nobody to ask"; status=1; fi
stop_cast

# An empty answer is a cancel.
start_fake_shell none
run_cast 1 0
if [ "$(cast_field response)" = 1 ]; then step PASS "2 an empty answer ends the session as cancelled"; else step FAIL "2 response was $(cast_field response)"; status=1; fi
if grep -q "label='" "$SHELL_LOG" && grep -q "kinds=1 " "$SHELL_LOG"; then step PASS "3 the shell got the app label and the kinds"; else step FAIL "3 the call was not as expected: $(cat "$SHELL_LOG")"; status=1; fi
stop_cast
kill "$SHELL_PID" 2>/dev/null || true; wait "$SHELL_PID" 2>/dev/null || true

# An output answer opens the stream.
start_fake_shell "output:$output"
run_cast 1 0
if [ "$(cast_field response)" = 0 ] && [ -n "$(cast_field node)" ]; then step PASS "4 an output answer opens the stream"; else step FAIL "4 Start returned: $(cat "$CAST_OUT")"; status=1; fi
exit $status
