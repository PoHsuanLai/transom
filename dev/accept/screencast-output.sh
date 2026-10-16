#!/usr/bin/env bash
# Acceptance script 'screencast-output': a PipeWire consumer receives the test pattern of a shared output.
# Prints PASS, SKIP or FAIL per step. Calls require_jail first.
#
# Owner: P2-T2.
#   exit 0 all steps passed, 1 a step failed, 2 not runnable (outside the jail, or not filled yet),
#   3 a tool is missing.
#
# Needs TRANSOM_BIN, TRANSOM_COMPOSITOR_ENV (a headless compositor with an output named in
# $TRANSOM_OUTPUT, default HEADLESS-1) and a consumer (gst-launch-1.0 with pipewiresrc).
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=dev/lib/jail.sh
source "$root/dev/lib/jail.sh"
require_jail
# shellcheck source=dev/lib/chain.sh
source "$root/dev/lib/chain.sh"
# shellcheck source=dev/lib/cast_check.sh
source "$root/dev/lib/cast_check.sh"
need_compositor screencast-output
output=${TRANSOM_OUTPUT:-HEADLESS-1}

env_file=$("$root/dev/up.sh" --env "$TRANSOM_COMPOSITOR_ENV")
# shellcheck source=/dev/null
source "$env_file"
trap 'stop_cast; stop_fake_shell; "$root/dev/down.sh" "$env_file" >/dev/null' EXIT
status=0

start_fake_shell "output:$output"
run_cast 1 2
if [ "$(cast_field response)" = 0 ] && [ -n "$(cast_field node)" ]; then
  step PASS "1 Start through the portal returned a node"
else
  step FAIL "1 Start returned: $(cat "$CAST_OUT")"; exit 1
fi

if grep -q "label='" "$SHELL_LOG"; then step PASS "2 the shell was asked"; else step FAIL "2 the shell was not asked"; status=1; fi

node=$(cast_field node)
rc=0
caps=$(frames_of "$node" 5) || rc=$?
[ "$rc" = 3 ] && exit 3
if grep -q "video/x-raw" <<<"$caps" && grep -q "width=(int)" <<<"$caps"; then
  step PASS "3 a consumer received frames: $(grep -o 'width=(int)[0-9]*, height=(int)[0-9]*' <<<"$caps" | head -n 1)"
else
  step FAIL "3 no frames reached the consumer"; status=1
  # What the consumer said, so a failure names its cause.
  tail -n 15 <<<"$caps" | sed 's/^/  consumer: /'
  explain_node "$node" | sed 's/^/  graph: /'
fi

token=$(cast_field token)
if [ -n "$token" ]; then step PASS "4 a restore token came back"; else step FAIL "4 no restore token"; status=1; fi
stop_cast
before=$(grep -c "^pick" "$SHELL_LOG" || true)
run_cast 1 2 "$token"
after=$(grep -c "^pick" "$SHELL_LOG" || true)
if [ "$(cast_field response)" = 0 ] && [ "$before" = "$after" ]; then
  step PASS "5 the restore token skipped the chooser"
else
  step FAIL "5 the chooser was asked again (picks $before -> $after)"; status=1
fi
exit $status
