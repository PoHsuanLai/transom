#!/usr/bin/env bash
# Acceptance script 'remote-desktop': RemoteDesktop events reach the compositor tagged as remote.
# Prints PASS, SKIP or FAIL per step. Calls require_jail first.
#
# Owner: P2-T3. Skeleton: the steps below are written as comments and exit 2 (not runnable yet)
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

# 1. dev/up.sh --env <compositor env file>.
# 2. CreateSession, SelectDevices (keyboard, pointer), Start, ConnectToEIS.
# 3. Send a key and a pointer motion; the compositor reports both with the remote origin.
# 4. dev/down.sh.

echo "SKIP remote-desktop: not filled yet (P2-T3)"
exit 2
