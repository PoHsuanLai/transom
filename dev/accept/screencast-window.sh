#!/usr/bin/env bash
# Acceptance script 'screencast-window': a PipeWire consumer receives one window, sized at logical size times scale, only on damage.
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

# 1. dev/up.sh --env <compositor env file> at scale 1.5; open a test window.
# 2. CreateSession, SelectSources (window), Start; the fake chooser picks the window.
# 3. Frames have size round(logical x 1.5); a static window yields no frames after the first.
# 4. Closing the window ends the session.
# 5. dev/down.sh.

echo "SKIP screencast-window: not filled yet (P2-T2)"
exit 2
