#!/usr/bin/env bash
# Acceptance script 'screencast-output': a PipeWire consumer receives the test pattern of a shared output.
# Prints PASS, SKIP or FAIL per step. Calls require_jail first.
#
# Owner: P2-T2. Skeleton: the steps below are written as comments and exit 2 (not runnable yet)
# until the lane fills them.
#   exit 0 all steps passed, 1 a step failed, 2 not runnable (outside the jail, or not filled yet),
#   3 a tool is missing.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=dev/lib/jail.sh
source "$root/dev/lib/jail.sh"
require_jail
# shellcheck source=dev/lib/chain.sh
source "$root/dev/lib/chain.sh"

# 1. dev/up.sh --env <compositor env file> (two outputs, a fake chooser answering for the first).
# 2. CreateSession, SelectSources (display), Start through the portal; take the node from the reply.
# 3. A pw-cat or pipewire-rs consumer on that node receives frames of the output size.
# 4. A restore token from the first Start lets a second Start skip the chooser.
# 5. dev/down.sh.

echo "SKIP screencast-output: not filled yet (P2-T2)"
exit 2
