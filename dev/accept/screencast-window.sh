#!/usr/bin/env bash
# Acceptance script 'screencast-window': a PipeWire consumer receives one window, sized at logical size times scale, only on damage.
# Prints PASS, SKIP or FAIL per step. Calls require_jail first.
#
# Owner: P2-T2.
#   exit 0 all steps passed, 1 a step failed, 2 not runnable (outside the jail, or not filled yet),
#   3 a tool is missing.
#
# Needs TRANSOM_BIN, TRANSOM_COMPOSITOR_ENV (a headless compositor at scale 1.5),
# TRANSOM_TEST_WINDOW (the identifier of an open window) and TRANSOM_TEST_WINDOW_SIZE (its
# logical size, WxH). A consumer: gst-launch-1.0 with pipewiresrc.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=dev/lib/jail.sh
source "$root/dev/lib/jail.sh"
require_jail
# shellcheck source=dev/lib/chain.sh
source "$root/dev/lib/chain.sh"
# shellcheck source=dev/lib/cast_check.sh
source "$root/dev/lib/cast_check.sh"
need_compositor screencast-window
if [ -z "${TRANSOM_TEST_WINDOW:-}" ] || [ -z "${TRANSOM_TEST_WINDOW_SIZE:-}" ]; then
  echo "SKIP screencast-window: set TRANSOM_TEST_WINDOW and TRANSOM_TEST_WINDOW_SIZE"
  exit 2
fi
scale=${TRANSOM_SCALE:-1.5}
lw=${TRANSOM_TEST_WINDOW_SIZE%x*}; lh=${TRANSOM_TEST_WINDOW_SIZE#*x}
want_w=$(awk -v n="$lw" -v s="$scale" 'BEGIN { printf "%d", n * s + 0.5 }')
want_h=$(awk -v n="$lh" -v s="$scale" 'BEGIN { printf "%d", n * s + 0.5 }')

env_file=$("$root/dev/up.sh" --env "$TRANSOM_COMPOSITOR_ENV")
# shellcheck source=/dev/null
source "$env_file"
trap 'stop_cast; kill "${SHELL_PID:-0}" 2>/dev/null || true; "$root/dev/down.sh" "$env_file" >/dev/null' EXIT
status=0

start_fake_shell "window:$TRANSOM_TEST_WINDOW"
run_cast 2 0
if [ "$(cast_field response)" = 0 ] && [ -n "$(cast_field node)" ]; then
  step PASS "1 Start for a window returned a node"
else
  step FAIL "1 Start returned: $(cat "$CAST_OUT")"; exit 1
fi

node=$(cast_field node)
caps=$(frames_of "$node" 1) || { rc=$?; [ "$rc" = 3 ] && exit 3; caps=""; }
if grep -q "width=(int)$want_w, height=(int)$want_h" <<<"$caps"; then
  step PASS "2 frames are ${want_w}x${want_h}, the logical size times $scale"
else
  step FAIL "2 expected ${want_w}x${want_h}; saw: $(grep -o 'width=(int)[0-9]*, height=(int)[0-9]*' <<<"$caps" | head -n 1)"; status=1
fi

# A window that does not change sends nothing after its first frame: asking for two frames from
# a static window must run into the timeout.
if frames_of "$node" 3 >/dev/null 2>&1; then
  step FAIL "3 a static window kept producing frames"; status=1
else
  step PASS "3 a static window produced no further frames"
fi
exit $status
