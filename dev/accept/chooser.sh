#!/usr/bin/env bash
# Acceptance script 'chooser': the source chooser handshake: a fake shell answers org.quire.Shell1.PickCaptureSource.
# Prints PASS, SKIP or FAIL per step. Calls require_jail first.
#
# Owner: P2-T2-CH. Skeleton: the steps below are written as comments and exit 2 (not runnable yet)
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

# 1. dev/up.sh; a fake shell claims org.quire.Shell on the bus and serves Shell1.
# 2. A ScreenCast Start calls PickCaptureSource with the app label, kinds and cursor modes.
# 3. An empty answer ends the session as cancelled; a window answer opens the stream.
# 4. dev/down.sh.

echo "SKIP chooser: not filled yet (P2-T2-CH)"
exit 2
